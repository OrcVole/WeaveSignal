<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The light account (ADR-0034): an email, a six-digit code, and for a new account a name, the
  // minimum-age declaration and the station's terms. Where the station allows guests, a guest
  // name instead, every message held for a moderator first. Reading never needs any of this.
  import { tick } from 'svelte';
  import type { ChatInfo, SignedIn } from '../../../../../packages/ui/chat.ts';
  import * as api from './api.ts';
  import { checkName, looksLikeEmail, tidyCode } from './model.ts';
  import Terms from './Terms.svelte';

  let { info, onsigned, onnotice }: { info: ChatInfo | null; onsigned: (s: SignedIn) => void; onnotice: () => void } = $props();

  type Step = 'start' | 'email' | 'code' | 'details' | 'guest';
  let step = $state<Step>('start');
  let email = $state('');
  let code = $state('');
  let name = $state('');
  let ageOk = $state(false);
  let terms = $state(false);
  let needs = $state<string[]>([]);
  let expiresMin = $state(10);
  let busy = $state(false);
  let errors = $state<Record<string, string>>({});
  let problem = $state<string | null>(null);
  let heading = $state<HTMLElement | undefined>();

  const guests = $derived(info?.rules.guests ?? false);
  const minAge = $derived(info?.min_age ?? 13);

  async function go(s: Step) {
    step = s;
    errors = {};
    problem = null;
    await tick();
    heading?.focus();
  }

  function focusFirst() {
    const f = ['email', 'code', 'name', 'age_ok', 'terms'].find((k) => errors[k]);
    if (f) void tick().then(() => document.getElementById(`si-${f}`)?.focus());
  }

  function fail(err: unknown) {
    if (err instanceof api.NeedsError) {
      // A new account: ask for everything a new account needs on the first 422.
      const listed = err.needs.length ? err.needs : [err.field ?? ''];
      needs = listed.includes('name') ? ['name', 'age_ok', 'terms'] : listed;
      const first = step !== 'details';
      if (first) {
        void go('details');
        return;
      }
      errors = { [err.field ?? 'name']: err.message };
      focusFirst();
    } else if (err instanceof api.ApiError && err.field) {
      // A code that expired while the new account's details were filled in: show the code again.
      if (err.field === 'code' && step === 'details') step = 'code';
      errors = { [err.field]: err.message };
      focusFirst();
    } else {
      problem = err instanceof Error ? err.message : String(err);
    }
  }

  async function start(e: SubmitEvent) {
    e.preventDefault();
    errors = {};
    problem = null;
    if (!looksLikeEmail(email)) {
      errors = { email: 'Enter your email address, like name@example.org.' };
      focusFirst();
      return;
    }
    busy = true;
    try {
      const r = await api.signinStart(email.trim());
      expiresMin = Math.max(1, Math.round(r.expires_s / 60));
      code = '';
      await go('code');
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  function detailErrors(): Record<string, string> {
    const out: Record<string, string> = {};
    if (needs.includes('name') || step === 'guest') {
      const c = checkName(name);
      if ('problem' in c) out.name = c.problem;
    }
    if ((needs.includes('age_ok') || step === 'guest') && !ageOk) out.age_ok = `Tick this to say you are ${minAge} or older.`;
    if ((needs.includes('terms') || step === 'guest') && !terms) out.terms = 'Tick this to accept the chat terms.';
    return out;
  }

  async function finish(e: SubmitEvent) {
    e.preventDefault();
    errors = {};
    problem = null;
    const c = tidyCode(code);
    if (c.length !== 6) {
      errors = { code: 'Enter the 6 digits from the email.' };
      focusFirst();
      return;
    }
    if (step === 'details') {
      errors = detailErrors();
      if (Object.keys(errors).length) {
        focusFirst();
        return;
      }
    }
    busy = true;
    try {
      const req: api.FinishRequest = { email: email.trim(), code: c };
      if (step === 'details') {
        if (needs.includes('name')) req.name = (checkName(name) as { name: string }).name;
        if (needs.includes('age_ok')) req.age_ok = ageOk;
        if (needs.includes('terms')) req.terms_version = info?.terms_version ?? '';
      }
      onsigned(await api.signinFinish(req));
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function asGuest(e: SubmitEvent) {
    e.preventDefault();
    errors = detailErrors();
    problem = null;
    if (Object.keys(errors).length) {
      focusFirst();
      return;
    }
    busy = true;
    try {
      onsigned(await api.guest({ name: (checkName(name) as { name: string }).name, age_ok: ageOk, terms_version: info?.terms_version ?? '' }));
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  const desc = (id: string, hint?: string) => [hint, errors[id] ? `si-${id}-err` : ''].filter(Boolean).join(' ') || undefined;
</script>

{#snippet err(id: string)}
  {#if errors[id]}<span class="err" id="si-{id}-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{errors[id]}</span>{/if}
{/snippet}

{#snippet details(guest: boolean)}
  {#if guest || needs.includes('name')}
    <div class="field">
      <label for="si-name">Your name in the chat</label>
      <span class="hint" id="si-name-hint">1 to 40 letters, numbers, spaces or _ - . '</span>
      <input id="si-name" type="text" autocomplete="nickname" maxlength="40" bind:value={name} aria-invalid={!!errors.name} aria-describedby={desc('name', 'si-name-hint')} data-chat="signin-name" />
      {@render err('name')}
    </div>
  {/if}
  {#if guest || needs.includes('age_ok')}
    <div class="field">
      <label class="check">
        <input id="si-age_ok" type="checkbox" bind:checked={ageOk} aria-invalid={!!errors.age_ok} aria-describedby={desc('age_ok')} data-chat="signin-age" />
        <span>I am {minAge} or older</span>
      </label>
      {@render err('age_ok')}
    </div>
  {/if}
  {#if guest || needs.includes('terms')}
    <div class="field">
      <Terms {info} id="si-terms-words" />
      <label class="check">
        <input id="si-terms" type="checkbox" bind:checked={terms} aria-invalid={!!errors.terms} aria-describedby={desc('terms', 'si-terms-words')} data-chat="signin-terms" />
        <span>I accept the chat terms<span class="small">Your name and messages may be read or shown on air, and kept with the show's recording.</span></span>
      </label>
      {@render err('terms')}
    </div>
  {/if}
{/snippet}

<section class="signin" aria-labelledby="si-h">
  {#if step === 'start'}
    <h3 id="si-h" tabindex="-1" bind:this={heading}>Join in</h3>
    <p class="small">Reading needs no account. To post, sign in with your email: we send you a code, no password.</p>
    <button type="button" class="btn primary wide" onclick={() => go('email')} data-chat="signin-open">Sign in to post</button>
    {#if guests}
      <button type="button" class="btn wide" onclick={() => go('guest')} data-chat="signin-guest">Post as a guest</button>
    {/if}
  {:else if step === 'email'}
    <form novalidate onsubmit={start}>
      <h3 id="si-h" tabindex="-1" bind:this={heading}>Sign in to post</h3>
      <div class="field">
        <label for="si-email">Your email</label>
        <span class="hint" id="si-email-hint">We send a 6-digit code to it. Other chatters never see it.</span>
        <input id="si-email" type="email" autocomplete="email" inputmode="email" bind:value={email} aria-invalid={!!errors.email} aria-describedby={desc('email', 'si-email-hint')} data-chat="signin-email" />
        {@render err('email')}
      </div>
      <button type="submit" class="btn primary wide" disabled={busy} data-chat="signin-submit">{busy ? 'Sending…' : 'Send the code'}</button>
      <button type="button" class="btn quiet wide" onclick={() => go('start')}>Not now</button>
    </form>
  {:else if step === 'code' || step === 'details'}
    <form novalidate onsubmit={finish}>
      <h3 id="si-h" tabindex="-1" bind:this={heading}>{step === 'details' ? 'Set up your chat name' : 'Enter your code'}</h3>
      {#if step === 'code'}
        <p class="small">We sent a code to <b>{email.trim()}</b>. It works for {expiresMin} minutes.</p>
      {:else}
        <p class="small">New here: choose the name other chatters see.</p>
      {/if}
      <div class="field" class:hidden={step === 'details'}>
        <label for="si-code">6-digit code</label>
        <input
          id="si-code"
          type="text"
          inputmode="numeric"
          autocomplete="one-time-code"
          maxlength="7"
          bind:value={code}
          aria-invalid={!!errors.code}
          aria-describedby={desc('code')}
          data-chat="signin-code"
        />
        {@render err('code')}
      </div>
      {#if step === 'details'}{@render details(false)}{/if}
      <button type="submit" class="btn primary wide" disabled={busy} data-chat="signin-submit">{busy ? 'Signing in…' : step === 'details' ? 'Join the chat' : 'Sign in'}</button>
      {#if step === 'code'}
        <button type="button" class="btn quiet wide" onclick={() => go('email')}>Use another email or send a new code</button>
      {:else}
        <button type="button" class="btn quiet wide" onclick={() => go('start')}>Not now</button>
      {/if}
    </form>
  {:else}
    <form novalidate onsubmit={asGuest}>
      <h3 id="si-h" tabindex="-1" bind:this={heading}>Post as a guest</h3>
      <p class="note"><svg aria-hidden="true"><use href="#g-hold" /></svg><span>A guest's messages wait a few seconds for a moderator before others see them.</span></p>
      {@render details(true)}
      <button type="submit" class="btn primary wide" disabled={busy} data-chat="signin-submit">{busy ? 'Joining…' : 'Join as a guest'}</button>
      <button type="button" class="btn quiet wide" onclick={() => go('start')}>Not now</button>
    </form>
  {/if}
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <button type="button" class="btn quiet wide" onclick={onnotice} data-chat="notice-open">Report something without an account</button>
</section>

<style>
  .signin,
  form {
    display: grid;
    gap: var(--s-3);
  }
  h3 {
    font-size: var(--fs-16);
  }
  h3:focus {
    outline: none;
  }
  h3:focus-visible {
    outline: var(--focus-ring);
  }
  #si-code {
    letter-spacing: 0.3em;
    font-variant-numeric: tabular-nums;
    max-width: 12rem;
  }
  .hidden {
    display: none;
  }
</style>
