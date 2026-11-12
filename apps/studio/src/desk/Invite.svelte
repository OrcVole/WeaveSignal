<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The desk's invite (ADR-0074): a quiet button at the head of Not on air, where the person
  // invited will appear, and its dialog. The dialog asks a name and a role (the DJ first: a DJ
  // joins off air), makes the invite (`POST /api/v1/invites`), then shows the link to send,
  // with Copy and how long it lasts. Panel name `desk.invite` (ADR-0070).
  //
  // A dialog owns the keyboard (keymap rule 5): App.svelte keeps the desk's keys out of it by
  // its panel name, so a name typed here never opens a fader. Esc closes it and the browser
  // puts the focus back on the button that opened it.
  import { base, makeInvite, sandboxed, type Invite, type InviteRole } from '../lib/api.ts';
  import { INVITE_ROLES, inviteProblem, inviteUrl, joinsLine, lastsLine, nameProblem } from '../lib/invite.ts';
  import { stationNow } from '../lib/studio.svelte.ts';

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let name = $state('');
  let role = $state<InviteRole>('dj');
  let busy = $state(false);
  let errors = $state<{ name?: string; form?: string }>({});
  let made = $state<Invite | null>(null);
  /** '' until Copy is pressed; then what happened, announced. */
  let copied = $state('');

  const url = $derived(made ? inviteUrl(made.link, base() || globalThis.location?.origin || '') : '');

  function fresh() {
    name = '';
    role = 'dj';
    errors = {};
    made = null;
    copied = '';
  }

  function open() {
    fresh();
    dialogEl?.showModal();
  }

  function another() {
    fresh();
    queueMicrotask(() => document.getElementById('inv-name')?.focus());
  }

  async function onMake(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    const problem = nameProblem(name);
    errors = problem ? { name: problem } : {};
    if (problem) return document.getElementById('inv-name')?.focus();
    busy = true;
    try {
      made = await makeInvite(role, name.trim());
      copied = '';
      queueMicrotask(() => document.getElementById('inv-copy')?.focus());
    } catch (err) {
      errors = { form: inviteProblem(err) };
    } finally {
      busy = false;
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(url);
      copied = 'Copied. Paste it into a message to them.';
    } catch {
      // Nothing to copy with (a webview may refuse): the link is selected in its box instead.
      copied = 'Could not copy from here. The link is selected in its box: copy it from there.';
      const box = document.getElementById('inv-link') as HTMLInputElement | null;
      box?.focus();
      box?.select();
    }
  }
</script>

<button type="button" class="btn quiet invite" aria-haspopup="dialog" onclick={open}>Invite someone…</button>

<!-- The pass is this dialog's only: closing it forgets the link (make another if it is lost). -->
<dialog bind:this={dialogEl} data-panel="desk.invite" aria-labelledby="inv-h" onclose={fresh}>
  {#if !made}
    <form onsubmit={onMake} novalidate>
      <h2 id="inv-h" data-panel-title>Invite someone to the room</h2>
      <p class="lede">You get a link to send them. They open it in a browser; no account, nothing to install.</p>
      <div class="field">
        <label for="inv-name">Their name</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          id="inv-name"
          type="text"
          bind:value={name}
          autofocus
          autocomplete="off"
          spellcheck="false"
          maxlength="120"
          aria-invalid={errors.name ? true : undefined}
          aria-describedby={errors.name ? 'inv-name-err inv-name-hint' : 'inv-name-hint'}
        />
        <span id="inv-name-hint" class="hint">As the desk shows it on their tile.</span>
        {#if errors.name}<p id="inv-name-err" class="err">{errors.name}</p>{/if}
      </div>
      <fieldset>
        <legend>They join as</legend>
        {#each INVITE_ROLES as r (r.role)}
          <label class="choice">
            <input type="radio" name="inv-role" value={r.role} bind:group={role} />
            <span><strong>{r.name}</strong><span class="what">{r.what}</span></span>
          </label>
        {/each}
      </fieldset>
      {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={() => dialogEl?.close()} disabled={busy}>Cancel <kbd>Esc</kbd></button>
        <button type="submit" class="btn primary" disabled={busy} aria-busy={busy || undefined}>{busy ? 'Creating…' : 'Create invitation'} <kbd>⏎</kbd></button>
      </div>
    </form>
  {:else}
    <div class="made">
      <h2 id="inv-h" data-panel-title>Send {made.name} this link</h2>
      <p class="lede">{joinsLine(made)}</p>
      <div class="link">
        <input id="inv-link" type="text" readonly value={url} aria-label="The invitation link for {made.name}" onfocus={(e) => e.currentTarget.select()} />
        <button id="inv-copy" type="button" class="btn primary" onclick={copy}>Copy link</button>
      </div>
      <p class="hint status" role="status" aria-live="polite">{copied}</p>
      <p class="hint lasts">
        {lastsLine(made, stationNow())} Anyone who has it can join as {made.name}, so send it to them alone. It is not shown again
        once you close this.
      </p>
      {#if sandboxed()}
        <p class="hint sandbox">In the sandbox the link opens nothing: the person a scenario is waiting for joins by themselves a few seconds from now.</p>
      {/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={another}>Invite someone else</button>
        <button type="button" class="btn" onclick={() => dialogEl?.close()}>Done <kbd>Esc</kbd></button>
      </div>
    </div>
  {/if}
</dialog>

<style>
  /* The button sits in the Not on air head, a row of 12 px capitals: it takes the row's size
     and none of its capitals, and stays to the right of the rule. */
  .invite {
    order: 2;
    flex: none;
    min-height: 1.5rem;
    padding: 0 var(--s-2);
    font-size: var(--fs-12);
    letter-spacing: 0;
    text-transform: none;
    color: var(--ink);
    border-color: var(--axis);
  }
  dialog {
    width: min(32rem, calc(100vw - 2 * var(--s-4)));
    max-height: calc(100vh - 2 * var(--s-4));
    overflow-y: auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
    font-size: var(--fs-14);
    letter-spacing: 0;
    text-transform: none;
    font-weight: 400;
  }
  /* Lighter than a settings dialog's: the lamp and the tiles stay readable behind it. */
  dialog::backdrop {
    background: var(--scrim);
  }
  form,
  .made {
    display: grid;
    gap: var(--s-3);
  }
  h2 {
    font-size: var(--fs-20);
    overflow-wrap: anywhere;
  }
  .lede {
    color: var(--ink-2);
    line-height: var(--lh-body);
  }
  .field {
    display: grid;
    gap: var(--s-1);
  }
  .field > label,
  legend {
    font-weight: 600;
  }
  input[type='text'] {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    width: 100%;
    min-width: 0;
    color-scheme: dark;
  }
  :global(:root[data-theme='light']) input {
    color-scheme: light;
  }
  input[readonly] {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
    text-overflow: ellipsis;
  }
  [aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .hint {
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  /* What a role means is the dialog's point (on air or off): the second ink, not the hint's. */
  .what {
    display: block;
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
  .lasts {
    color: var(--ink-2);
  }
  .status {
    color: var(--ink);
    min-height: calc(var(--fs-13) * var(--lh-body));
  }
  .sandbox {
    border-left: 3px solid var(--axis);
    padding-left: var(--s-2);
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  fieldset {
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3);
    display: grid;
    gap: var(--s-2);
  }
  .choice {
    display: flex;
    gap: var(--s-2);
    align-items: baseline;
    line-height: var(--lh-body);
  }
  .link {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--s-2);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-1);
  }
</style>
