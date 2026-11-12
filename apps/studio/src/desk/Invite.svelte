<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The desk's invitation (ADR-0074): a quiet button at the head of Not on air, where the
  // person invited will appear, and its dialog. The dialog asks a name and a role (the DJ first:
  // a DJ joins off air), makes the invitation (`POST /api/v1/invites`), then shows the link to
  // send, with Copy and how long it lasts. Panel name `desk.invite` (ADR-0070).
  //
  // The show's step decides the roles (ADR-0271): a guest and a presenter from Guests, the next
  // show's DJ at Full. With `own` it is the Solo DJ's one button instead (ADR-0274): Join with
  // my microphone… makes the signed-in presenter's own link, with no name to type and no role
  // to choose, and opens the dialog on its second page, with Open in browser and Copy link.
  //
  // A dialog owns the keyboard (keymap rule 5): App.svelte keeps the desk's keys out of it by
  // its panel name, so a name typed here never opens a fader. Esc closes it and the browser
  // puts the focus back on the button that opened it.
  import { base, makeInvite, makeOwnInvite, openInBrowser, sandboxed, type Invite, type InviteRole } from '../lib/api.ts';
  import { DJ_COMES_WITH_FULL, OWN_LINK, inviteProblem, inviteUrl, joinsLine, lastsLine, nameProblem, rolesAt } from '../lib/invite.ts';
  import { stationNow, studio } from '../lib/studio.svelte.ts';
  import { stationStep } from '../lib/steps.ts';

  let { own = false }: { /** The Solo DJ's own microphone link, in place of the dialog's first page. */ own?: boolean } = $props();

  /** The roles the station's step lets the desk invite; the first is the one picked. */
  const roles = $derived(rolesAt(stationStep(studio.view)));

  let dialogEl = $state<HTMLDialogElement | null>(null);
  let name = $state('');
  let role = $state<InviteRole>('guest');
  let busy = $state(false);
  let errors = $state<{ name?: string; form?: string }>({});
  let made = $state<Invite | null>(null);
  /** '' until Copy is pressed; then what happened, announced. */
  let copied = $state('');

  const url = $derived(made ? inviteUrl(made.link, base() || globalThis.location?.origin || '') : '');

  function fresh() {
    name = '';
    role = roles[0]?.role ?? 'guest';
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

  /** Join with my microphone…: the link is made at once, and the dialog opens to say what to do
   *  with it. A refusal is the station's own sentence, shown under the button. */
  async function openOwn() {
    if (busy) return;
    fresh();
    busy = true;
    try {
      made = await makeOwnInvite();
      dialogEl?.showModal();
      queueMicrotask(() => document.getElementById('inv-open')?.focus());
    } catch (err) {
      errors = { form: inviteProblem(err) };
    } finally {
      busy = false;
    }
  }

  async function openLink() {
    copied = (await openInBrowser(url)) ? 'Opened in your browser. Allow the microphone if it asks, then press Join the studio.' : OWN_LINK.notOpened;
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(url);
      copied = own ? 'Copied. Paste it into a browser on this computer.' : 'Copied. Paste it into a message to them.';
    } catch {
      // Nothing to copy with (a webview may refuse): the link is selected in its box instead.
      copied = 'Could not copy from here. The link is selected in its box: copy it from there.';
      const box = document.getElementById('inv-link') as HTMLInputElement | null;
      box?.focus();
      box?.select();
    }
  }
</script>

{#if own}
  <button type="button" class="btn primary" aria-haspopup="dialog" disabled={busy} aria-busy={busy || undefined} aria-describedby={errors.form ? 'inv-own-err' : undefined} onclick={openOwn}
    >{busy ? 'Making your link…' : OWN_LINK.button}</button
  >
  {#if errors.form && !made}<p id="inv-own-err" class="err own-err" role="alert">{errors.form}</p>{/if}
{:else}
  <button type="button" class="btn quiet invite" aria-haspopup="dialog" onclick={open}>Invite someone…</button>
{/if}

<!-- The pass is this dialog's only: closing it forgets the link (make another if it is lost). -->
<dialog bind:this={dialogEl} data-panel="desk.invite" aria-labelledby="inv-h" onclose={fresh}>
  {#if made && own}
    <div class="made">
      <h2 id="inv-h" data-panel-title>{OWN_LINK.title}</h2>
      <p class="lede">{OWN_LINK.lede}</p>
      <div class="link">
        <input id="inv-link" type="text" readonly value={url} aria-label="Your microphone link" onfocus={(e) => e.currentTarget.select()} />
        <button id="inv-open" type="button" class="btn primary" onclick={openLink}>{OWN_LINK.open}</button>
      </div>
      <p class="hint status" role="status" aria-live="polite">{copied}</p>
      <p class="hint lasts">{lastsLine(made, stationNow())} Anyone who has it joins as you, so keep it to yourself. The same button makes it again.</p>
      {#if sandboxed()}
        <p class="hint sandbox">In the sandbox the link opens nothing: the browser tab you speak through is the small panel in the bar at the top.</p>
      {/if}
      <div class="acts">
        <button id="inv-copy" type="button" class="btn" onclick={copy}>Copy link</button>
        <button type="button" class="btn quiet" onclick={() => dialogEl?.close()}>Done <kbd>Esc</kbd></button>
      </div>
    </div>
  {:else if !made}
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
        {#each roles as r (r.role)}
          <label class="choice">
            <input type="radio" name="inv-role" value={r.role} bind:group={role} />
            <span><strong>{r.name}</strong><span class="what">{r.what}</span></span>
          </label>
        {/each}
        {#if !roles.some((r) => r.role === 'dj')}<p class="hint">{DJ_COMES_WITH_FULL}</p>{/if}
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
  /* The station's sentence under the Solo stage's button, when it refuses the DJ's own link. */
  .own-err {
    max-width: 26rem;
    text-align: left;
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
