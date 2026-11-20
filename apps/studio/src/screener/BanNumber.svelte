<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Ban a phone caller's number (`screener.ban-number`; 1.2, PLAN 17f): from their card. The
  // station keeps the last four digits and a key made from the number, never the number; the
  // screener gives a reason for the next screener to read, and how long. The caller stays in
  // the queue: dropping them is its own act, with its own two presses.
  import { tick } from 'svelte';
  import { placeError } from '../lib/account.ts';
  import { BAN_LENGTHS, banCaller, banProblem } from '../lib/phone.ts';

  let { onBanned }: { onBanned: (line: string) => void } = $props();

  let el = $state<HTMLDialogElement | null>(null);
  let who = $state<{ identity: string; line: string } | null>(null);
  let reason = $state('');
  let days = $state<number | null>(30);
  let error = $state('');
  let fieldError = $state('');
  let busy = $state(false);

  export async function open(identity: string, line: string) {
    who = { identity, line };
    reason = '';
    days = 30;
    error = '';
    fieldError = '';
    await tick();
    el?.showModal();
    document.getElementById('bn-reason')?.focus();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!who || busy) return;
    fieldError = banProblem(reason) ?? '';
    error = '';
    if (fieldError) return document.getElementById('bn-reason')?.focus();
    busy = true;
    try {
      await banCaller(who.identity, reason.trim(), days);
      const line = who.line;
      el?.close();
      onBanned(line);
    } catch (err) {
      error = placeError(err, []).message;
    } finally {
      busy = false;
    }
  }
</script>

<dialog data-panel="screener.ban-number" bind:this={el} aria-labelledby="bn-h" aria-describedby="bn-d" onclose={() => (who = null)}>
  <h2 id="bn-h" data-panel-title>{who ? `Ban ${who.line}?` : ''}</h2>
  {#if who}
    <p id="bn-d">When this number rings again its caller is marked BANNED BEFORE, with your reason. The station keeps the last four digits and a key made from the number, never the number. Caller ID can be faked, so a ban is a help, not a lock.</p>
    <form onsubmit={submit} novalidate>
      <div class="f">
        <label for="bn-reason">Why</label>
        <input id="bn-reason" type="text" bind:value={reason} maxlength="200" autocomplete="off" aria-invalid={fieldError ? true : undefined} aria-describedby={fieldError ? 'bn-reason-err' : undefined} />
        {#if fieldError}<p id="bn-reason-err" class="err">{fieldError}</p>{/if}
      </div>
      <div class="f">
        <label for="bn-days">For how long</label>
        <select id="bn-days" value={days ?? 0} onchange={(e) => (days = Number(e.currentTarget.value) || null)}>
          {#each BAN_LENGTHS as l (l.label)}<option value={l.days ?? 0}>{l.label}</option>{/each}
        </select>
      </div>
      <p class="small">This caller stays in the queue. To end the call, drop them: <kbd>⌃D</kbd><kbd>⌃D</kbd>.</p>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" disabled={busy} onclick={() => el?.close()}>Cancel</button>
        <button type="submit" class="btn danger-hollow" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Banning…' : 'Ban this number'}</button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  /* this panel's own inputs: the styles are scoped, so each settings panel carries them */
  dialog :global(input:not([type='checkbox'], [type='radio'], [type='file'])),
  dialog :global(select) {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
    min-width: 0;
    max-width: 100%;
  }
  :global(:root[data-theme='light']) dialog :global(select),
  :global(:root[data-theme='light']) dialog :global(input) {
    color-scheme: light;
  }
  dialog :global(input[type='checkbox']),
  dialog :global(input[type='radio']) {
    accent-color: var(--accent);
  }
  dialog :global([aria-invalid='true']) {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  dialog :global(input:disabled),
  dialog :global(select:disabled) {
    opacity: 0.6;
  }
  dialog {
    width: min(30rem, calc(100vw - 2rem));
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    background: var(--surface);
    color: var(--ink);
    padding: var(--s-4);
    box-shadow: var(--e-3, var(--e-1));
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.5);
  }
  h2 {
    font-size: var(--fs-20);
    margin-bottom: var(--s-2);
  }
  p {
    font-size: var(--fs-14);
    line-height: var(--lh-body);
    color: var(--ink-2);
  }
  form {
    display: grid;
    gap: var(--s-3);
    margin-top: var(--s-3);
  }
  .f {
    display: grid;
    gap: var(--s-1);
  }
  label {
    font-size: var(--fs-13);
    font-weight: 600;
  }
  .f input {
    width: 100%;
  }
  .f select {
    justify-self: start;
  }
  .err {
    color: var(--status-serious);
    font-size: var(--fs-13);
  }
  .small {
    font-size: var(--fs-13);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: var(--s-2);
  }
</style>
