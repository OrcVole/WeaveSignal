<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The four dialogs of a show key (ADR-0280, ADR-0284; hosts-in-their-own-studios.md sections
  // 3.1 and 4.3), kept in one place because three screens open them: Settings › Own studios,
  // the show editor's Comes from, and Your show.
  //
  //   settings.hosts.setup     the show, who it is for, how they will send
  //   settings.hosts.details   what their software asks for; the secret, shown this once
  //   settings.hosts.new-key   a new key in place of one: now, or when they disconnect
  //   settings.hosts.end       ending a key, and what stops with it
  //
  // The secret lives in this component's state only, from the station's reply until *I have
  // sent them*. That button is the details' only way out: Esc does not close it, and a close
  // the browser makes anyway (a second Esc in Chromium) opens it again with the secret still
  // there. Nothing here is kept after it, and nothing is written to storage.
  //
  // The monitor link (design section 4.5) is given whole with the details: the station's public
  // address, then `/monitor/#` and the link's own secret, with Copy, and in the text to send.
  // One thing outlives the dialog, in memory only: when the key is the signed-in person's own,
  // this window holds their monitor link (`lib/monitor.svelte.ts`) so Open my monitor in Your
  // show works until the window closes. The station can never give that link again.
  import { tick } from 'svelte';
  import { base, endShowKey, makeShowKey, openInBrowser, renewShowKey, type KeyRow, type MadeKey, type SourceDetails, type SourceWay, type StaffAccount } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { account } from '../lib/account.svelte.ts';
  import { dropMonitor, holdMonitor } from '../lib/monitor.svelte.ts';
  import { loadPublicAddress, sched } from '../lib/schedule.svelte.ts';
  import {
    LABEL_MAX,
    MONITOR_BY_HAND,
    SECOND_KEY_NOTE,
    TELL_HOST,
    WAY_LINE,
    WAY_WORD,
    detailBlocks,
    detailsText,
    hostName,
    labelProblem,
    monitorLink,
    possessive,
    showOption,
    stationHost,
    type DetailField,
    type ShowChoice,
  } from '../lib/hosts.ts';

  let {
    shows,
    staff = null,
    onChanged,
    onNote,
  }: {
    /** The shows a key can be made for. */
    shows: ShowChoice[];
    /** The staff accounts, where this window may read them (an admin's). */
    staff?: StaffAccount[] | null;
    /** A key was made or ended: the caller reads its lists again. */
    onChanged: () => void | Promise<void>;
    /** One sentence for the caller's status line. */
    onNote?: (note: string) => void;
  } = $props();

  /** The address this studio reaches the station at, for a station that gives none of its own. */
  const studioAddress = () => base() || globalThis.location?.origin || '';

  // ---- 1. set up a host ----

  let setupEl = $state<HTMLDialogElement | null>(null);
  let draft = $state({ show: '', label: '', staffId: '' as string, hint: 'icecast' as SourceWay });
  /** The show is given (the show editor, Your show) and not asked. */
  let fixed = $state(false);
  let errors = $state<{ show?: string; label?: string; form?: string }>({});
  let making = $state(false);

  const picked = $derived(shows.find((s) => s.slug === draft.show) ?? null);
  const people = $derived((staff ?? []).filter((a) => a.disabled_ms === null));

  /** Set up a host…: for any show, or for the one given. */
  export function setup(o: { show?: string; label?: string; staffId?: string | null; fixed?: boolean; hint?: SourceWay } = {}) {
    draft = { show: o.show ?? (shows.length === 1 ? shows[0].slug : ''), label: o.label ?? '', staffId: o.staffId ?? '', hint: o.hint ?? 'icecast' };
    fixed = !!o.fixed;
    errors = {};
    setupEl?.showModal();
    queueMicrotask(() => document.getElementById(o.show || shows.length === 1 ? 'hk-label' : 'hk-show')?.focus());
  }

  function pickStaff(id: string) {
    draft.staffId = id;
    const a = people.find((x) => x.id === id);
    if (a) draft.label = a.name || a.username;
  }

  async function onMake(e: SubmitEvent) {
    e.preventDefault();
    if (making) return;
    const errs: typeof errors = {};
    if (!draft.show) errs.show = 'Choose the show the key is for. A key works for one show only.';
    const label = labelProblem(draft.label);
    if (label) errs.label = label;
    errors = errs;
    if (errs.show) return document.getElementById('hk-show')?.focus();
    if (errs.label) return document.getElementById('hk-label')?.focus();
    making = true;
    try {
      const made = await makeShowKey(draft.show, { label: draft.label.trim(), hint: draft.hint, staff_id: draft.staffId || null });
      setupEl?.close();
      await showMade(made, picked?.name ?? draft.show);
    } catch (err) {
      const placed = placeError(err, ['label', 'hint', 'staff_id'] as const);
      errors = placed.field === 'label' ? { label: placed.message } : { form: placed.message };
      if (placed.field === 'label') document.getElementById('hk-label')?.focus();
    } finally {
      making = false;
    }
  }

  // ---- 2. the details ----

  let detailsEl = $state<HTMLDialogElement | null>(null);
  /** What the details show. With `made`, the secret is in them and the dialog has one way out. */
  let shown = $state<{ label: string; showName: string; hint: SourceWay; details: SourceDetails; made: MadeKey | null; monitor?: 'open' | 'closed' | null } | null>(null);
  let copied = $state<string | null>(null);
  /** Copy everything could not reach the clipboard: the text is drawn, selected, to copy by hand. */
  let byHand = $state(false);
  /** A field could not be copied (a webview may refuse): what to do instead. */
  let copyNote = $state('');

  const host = $derived(shown ? stationHost(shown.details, studioAddress()) : '');
  const blocks = $derived(shown ? detailBlocks(shown.details, host, shown.made?.secret ?? null) : null);
  const order = $derived<SourceWay[]>(shown?.hint === 'srt' ? ['srt', 'icecast'] : ['icecast', 'srt']);
  /** The whole monitor link, made from the station's public address; null when it has none. */
  const monitor = $derived(shown?.made ? monitorLink(sched.publicUrl, shown.made.monitor_secret) : null);
  /** The key is the signed-in person's own: the monitor is theirs to open from here. */
  const mine = $derived(!!shown?.made?.key.staff_id && shown.made.key.staff_id === account.me?.account?.id);
  const everything = $derived(shown?.made ? detailsText(shown.made, shown.showName, host, monitor) : '');
  const first = $derived(shown ? hostName(shown.label) : '');

  async function showMade(made: MadeKey, showName: string) {
    // The link is made from the station's public address, which a window that has not opened
    // the Schedule tab has not read yet.
    const address = await loadPublicAddress();
    const link = monitorLink(address, made.monitor_secret);
    if (link && made.key.staff_id && made.key.staff_id === account.me?.account?.id) holdMonitor(made.key.key_id, link);
    shown = { label: made.key.label, showName, hint: made.key.hint, details: made.details, made };
    copied = null;
    byHand = false;
    copyNote = '';
    // The lists are read again behind the dialog: the row is there when it closes.
    void onChanged();
    await tick();
    detailsEl?.showModal();
    // Not on the way out: a second ⏎ after Make the key must not close what cannot be shown again.
    queueMicrotask(() => document.getElementById('hk-all')?.focus());
  }

  /** Details…: a key made earlier, with everything but the secret, and whether its monitor link
   *  is open now (`monitor`: null when the host is not connected, so nothing is known). */
  export async function details(row: KeyRow, showName: string, monitor: 'open' | 'closed' | null = null) {
    shown = { label: row.label, showName, hint: row.hint, details: row.details, made: null, monitor };
    copied = null;
    byHand = false;
    copyNote = '';
    await tick();
    detailsEl?.showModal();
  }

  async function copy(id: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = id;
      copyNote = '';
    } catch {
      // Nothing to copy with: the value is selected in its box, to copy by hand.
      copied = null;
      if (id === 'hk-all') {
        byHand = true;
        copyNote = '';
        await tick();
      } else copyNote = 'Could not copy from here. The value is selected in its box: copy it from there.';
      const el = document.getElementById(id === 'hk-all' ? 'hk-all-text' : id) as HTMLInputElement | HTMLTextAreaElement | null;
      el?.focus();
      el?.select();
    }
  }

  /** Open my monitor: the person's own monitor link, in their own browser (the packaged app's
   *  window cannot play the room on Linux). */
  async function openMonitor() {
    if (!monitor) return;
    copyNote = (await openInBrowser(monitor)) ? 'Your monitor is open in your browser. Press Start listening there.' : 'Could not open a browser from here. Copy the link and paste it into your browser.';
  }

  /** I have sent them: the secret is let go, and cannot be shown again. */
  function sent() {
    const was = shown;
    shown = null;
    copied = null;
    byHand = false;
    copyNote = '';
    detailsEl?.close();
    if (was?.made) onNote?.(`${possessive(was.label)} key for ${was.showName} is ready. Its secret is not kept anywhere you can read it again: make a new key if it is lost.`);
  }

  /** The browser closed the dialog while the secret was still to send: open it again. */
  function keepOpen() {
    if (shown?.made && detailsEl && !detailsEl.open) detailsEl.showModal();
  }

  // ---- 3. a new key ----

  let renewEl = $state<HTMLDialogElement | null>(null);
  let renewing = $state<{ row: KeyRow; slug: string; showName: string; connected: boolean; onAir: boolean } | null>(null);
  let when = $state<'on_disconnect' | 'now'>('on_disconnect');
  let busy = $state(false);
  let problem = $state('');

  /** New key…: asked first; with a connection open, asked when the old one ends. */
  export function renew(row: KeyRow, show: { slug: string; name: string }, status: { connected: boolean; onAir: boolean }) {
    renewing = { row, slug: show.slug, showName: show.name, ...status };
    when = 'on_disconnect';
    problem = '';
    renewEl?.showModal();
  }

  async function confirmRenew() {
    const r = renewing;
    if (!r || busy) return;
    busy = true;
    problem = '';
    try {
      const made = await renewShowKey(r.slug, r.row.key_id, r.connected ? when : 'now');
      // The old key's monitor link ends with it.
      dropMonitor(r.row.key_id);
      renewEl?.close();
      renewing = null;
      await showMade(made, r.showName);
    } catch (e) {
      problem = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }

  // ---- 4. end a key ----

  let endEl = $state<HTMLDialogElement | null>(null);
  let ending = $state<{ row: KeyRow; slug: string; showName: string; connected: boolean; onAir: boolean; last: boolean } | null>(null);

  /** End key…: asked first, with what stops. */
  export function end(row: KeyRow, show: { slug: string; name: string }, status: { connected: boolean; onAir: boolean }, last: boolean) {
    ending = { row, slug: show.slug, showName: show.name, ...status, last };
    problem = '';
    endEl?.showModal();
  }

  async function confirmEnd() {
    const r = ending;
    if (!r || busy) return;
    busy = true;
    problem = '';
    try {
      await endShowKey(r.slug, r.row.key_id);
      dropMonitor(r.row.key_id);
      endEl?.close();
      ending = null;
      onNote?.(`${possessive(r.row.label)} key for ${r.showName} has ended. Their software is refused from now on.${r.last ? ` ${r.showName} comes from the station’s room again.` : ''}`);
      await onChanged();
    } catch (e) {
      problem = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }
</script>

<dialog data-panel="settings.hosts.setup" bind:this={setupEl} aria-labelledby="hk-h">
  <form onsubmit={onMake} novalidate>
    <h2 id="hk-h" data-panel-title>Set up a host</h2>
    <div class="hk-field">
      <label for="hk-show">Show</label>
      {#if fixed && picked}
        <output id="hk-show" class="given">{picked.name}</output>
      {:else}
        <select id="hk-show" bind:value={draft.show} onchange={() => errors.show && (errors = { ...errors, show: undefined })} aria-invalid={errors.show ? true : undefined} aria-describedby={errors.show ? 'hk-show-err hk-show-hint' : 'hk-show-hint'}>
          <option value="" disabled>Choose a show</option>
          {#each shows as s (s.slug)}<option value={s.slug}>{showOption(s.name, s.holders)}</option>{/each}
        </select>
      {/if}
      <span id="hk-show-hint" class="hint">The key works for this show only, at its times.</span>
      {#if errors.show}<p id="hk-show-err" class="err">{errors.show}</p>{/if}
      {#if picked?.holders.length}<p class="aside" data-testid="second-key">{SECOND_KEY_NOTE}</p>{/if}
    </div>
    <div class="hk-field">
      <label for="hk-label">Who is it for</label>
      <input
        id="hk-label"
        type="text"
        bind:value={draft.label}
        oninput={() => errors.label && (errors = { ...errors, label: undefined })}
        autocomplete="off"
        maxlength={LABEL_MAX}
        aria-invalid={errors.label ? true : undefined}
        aria-describedby={errors.label ? 'hk-label-err hk-label-hint' : 'hk-label-hint'}
      />
      <span id="hk-label-hint" class="hint">Their name on the desk. Each person gets a key of their own, so one can be ended without stopping the others.</span>
      {#if errors.label}<p id="hk-label-err" class="err">{errors.label}</p>{/if}
    </div>
    {#if people.length}
      <div class="hk-field">
        <label for="hk-staff">A member of staff <span class="opt">optional</span></label>
        <select id="hk-staff" value={draft.staffId} onchange={(e) => pickStaff(e.currentTarget.value)}>
          <option value="">Not a member of staff</option>
          {#each people as a (a.id)}<option value={a.id}>{a.name || a.username}</option>{/each}
        </select>
        <span class="hint">Ties the key to their account, and fills in their name.</span>
      </div>
    {/if}
    <fieldset>
      <legend>How they will send</legend>
      {#each ['icecast', 'srt'] as const as w (w)}
        <label class="way" class:on={draft.hint === w}>
          <input type="radio" name="hk-hint" value={w} checked={draft.hint === w} onchange={() => (draft.hint = w)} />
          <span><strong>{WAY_WORD[w]}.</strong> {WAY_LINE[w]}</span>
        </label>
      {/each}
      <span class="hint">A hint, not a lock: the one key works either way, and the details give both.</span>
    </fieldset>
    {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => setupEl?.close()} disabled={making}>Cancel</button>
      <button type="submit" class="btn primary" disabled={making} aria-busy={making || undefined}>{making ? 'Making…' : 'Make the key'}</button>
    </div>
  </form>
</dialog>

{#snippet field(f: DetailField, id: string, way: string)}
  <div class="f">
    <span class="fl" id="{id}-l">{f.label}</span>
    {#if f.words}
      <span class="fw" class:gone={!f.copy && f.label !== 'Key length'}>{f.value}</span>
    {:else}
      <input {id} type="text" readonly value={f.value} aria-labelledby="{id}-l" spellcheck="false" onfocus={(e) => e.currentTarget.select()} />
      {#if f.copy}
        <button type="button" class="btn sm" aria-label="Copy the {way} {f.label.toLowerCase()}" onclick={() => copy(id, f.value)}>{copied === id ? 'Copied' : 'Copy'}</button>
      {/if}
    {/if}
  </div>
{/snippet}

<dialog
  class="details"
  data-panel="settings.hosts.details"
  bind:this={detailsEl}
  aria-labelledby="hd-h"
  aria-describedby="hd-d"
  oncancel={(e) => shown?.made && e.preventDefault()}
  onclose={keepOpen}
>
  <!-- The heading is always drawn, so the panel's name plate (ADR-0070) is bound to it and not to
       the dialog itself, which would leave the plate showing after the dialog closes. -->
  <header>
    <h2 id="hd-h" data-panel-title>{shown ? `${possessive(first)} details for ${shown.showName}` : ''}</h2>
    {#if shown}
      <p id="hd-d">
        {#if shown.made}
          <strong>This is the only time the secret is shown.</strong> The station keeps a scrambled form, so it cannot be read again.
        {:else}
          Everything but the secret, which was shown once, when the key was made. If {first} has lost it, make a new key.
        {/if}
      </p>
    {/if}
  </header>
  {#if shown && blocks}
    <div class="body">
      <div class="ways">
        {#each order as w, i (w)}
          <section class="waybox" aria-labelledby="hd-{w}">
            <h3 id="hd-{w}">
              {WAY_WORD[w]}
              {#if i === 0}<span class="tag">how {first} said they will send</span>{/if}
            </h3>
            <p class="small">{w === 'icecast' ? 'Most radio software. About 2 s to the station.' : 'OBS and other software that offers it. Under half a second.'}</p>
            {#each blocks[w] as f, n (f.label)}{@render field(f, `hd-${w}-${n}`, WAY_WORD[w])}{/each}
            {#if w === 'icecast'}
              <p class="small">The password crosses the internet as every Icecast password does. Prefer SRT on a network {first} does not trust.</p>
            {:else}
              <p class="small">The passphrase encrypts the sound, so the secret never crosses in clear.</p>
            {/if}
          </section>
        {/each}
      </div>
      {#if shown.made}
        <section class="waybox monitor" aria-labelledby="hd-mon" data-testid="monitor-link">
          <h3 id="hd-mon">Monitor link</h3>
          {#if monitor}
            {@render field({ label: 'Link', value: monitor, copy: true, secret: true }, 'hd-mon-0', 'monitor')}
            <p class="small">
              A page {first} keeps open in a browser beside their software: it says when they are on air, plays the station without them, and carries your cue and Talk. The link holds a
              second secret, made and ended with the key and shown this once too.
            </p>
            {#if mine}
              <div class="mon-open"><button type="button" class="btn" onclick={openMonitor}>Open my monitor</button><span class="small">Opens in your browser. This window can open it again from Your show, until it is closed.</span></div>
            {/if}
          {:else}
            {@render field({ label: 'Its secret', value: shown.made.monitor_secret, copy: true, secret: true }, 'hd-mon-0', 'monitor link')}
            <p class="small" data-testid="monitor-by-hand">
              {MONITOR_BY_HAND} This station’s settings give no public address (<code>publishing.public_url</code>), so the studio cannot write the link out. The page plays the station without {first}, and carries
              your cue and Talk.
            </p>
          {/if}
        </section>
      {:else if shown.monitor}
        <section class="waybox monitor" aria-labelledby="hd-mon" data-testid="monitor-state">
          <h3 id="hd-mon">Monitor link <span class="tag">{shown.monitor === 'open' ? 'open now' : 'not open'}</span></h3>
          <p class="small">
            {shown.monitor === 'open' ? `${first} has the monitor link open: they hear the station, your cue and Talk.` : `${first} is connected without the monitor link open: they hear nothing from the station, and Talk is off.`}
            The link was shown once, with the secret. If {first} has lost it, make a new key.
          </p>
        </section>
      {/if}
      <p class="tell"><strong>Tell {first}:</strong> {TELL_HOST.charAt(0).toLowerCase() + TELL_HOST.slice(1)}</p>
      {#if byHand}
        <div class="hk-field">
          <label for="hk-all-text">Everything, as text</label>
          <textarea id="hk-all-text" readonly rows="8" spellcheck="false">{everything}</textarea>
          <span class="hint">This window could not reach the clipboard. Select it all and copy it by hand.</span>
        </div>
      {/if}
    </div>
    <div class="acts">
      <span class="small copy-note" role="status" aria-live="polite">{copyNote}</span>
      {#if shown.made}
        <button type="button" id="hk-all" class="btn" onclick={() => copy('hk-all', everything)}>{copied === 'hk-all' ? 'Copied everything' : 'Copy everything as text'}</button>
        <button type="button" class="btn primary" onclick={sent}>I have sent them</button>
      {:else}
        <button type="button" class="btn primary" onclick={sent}>Close</button>
      {/if}
    </div>
  {/if}
</dialog>

<dialog data-panel="settings.hosts.new-key" bind:this={renewEl} aria-labelledby="hn-h" aria-describedby="hn-d">
  <h2 id="hn-h" data-panel-title>{renewing ? `Make a new key for ${hostName(renewing.row.label)}?` : ''}</h2>
  {#if renewing}
    {@const who = hostName(renewing.row.label)}
    <p id="hn-d">The key {who} has now stops working. {who} needs the new details before the next {renewing.showName}.</p>
    {#if renewing.connected}
      <fieldset>
        <legend>{who} is connected with the old key. It ends</legend>
        <label class="way" class:on={when === 'on_disconnect'}>
          <input type="radio" name="hn-when" checked={when === 'on_disconnect'} onchange={() => (when = 'on_disconnect')} />
          <span><strong>When {who} disconnects.</strong> Nothing changes on air. The old key works until its connection closes.</span>
        </label>
        <label class="way" class:on={when === 'now'}>
          <input type="radio" name="hn-when" checked={when === 'now'} onchange={() => (when = 'now')} />
          <span
            ><strong>Now.</strong> The connection is closed at once.
            {#if renewing.onAir}{who} is on air: listeners hear whoever else is on air, or silence until automation covers.{/if}</span
          >
        </label>
      </fieldset>
    {/if}
    {#if problem}<p class="err" role="alert">{problem}</p>{/if}
    <div class="acts">
      <button type="button" class="btn" onclick={() => renewEl?.close()} disabled={busy}>Keep the key</button>
      <button type="button" class="btn primary" onclick={confirmRenew} disabled={busy} aria-busy={busy || undefined}>{busy ? 'Making…' : 'Make a new key'}</button>
    </div>
  {/if}
</dialog>

<dialog data-panel="settings.hosts.end" bind:this={endEl} aria-labelledby="he-h" aria-describedby="he-d">
  <h2 id="he-h" data-panel-title>{ending ? `End ${possessive(hostName(ending.row.label))} key for ${ending.showName}?` : ''}</h2>
  {#if ending}
    {@const who = hostName(ending.row.label)}
    <p id="he-d">
      {possessive(who)} software is refused from now on{ending.connected ? ', and the connection that is open is closed' : ''}. {ending.showName} stays in the schedule{ending.last
        ? '; with no key left it comes from the station’s room again.'
        : '.'}
    </p>
    {#if ending.onAir}
      <p class="warn" data-testid="end-on-air"><strong>{who} is on air with this key.</strong> Listeners will hear whoever else is on air, or silence until automation covers.</p>
    {/if}
    {#if problem}<p class="err" role="alert">{problem}</p>{/if}
    <div class="acts">
      <button type="button" class="btn" onclick={() => endEl?.close()} disabled={busy}>Keep the key</button>
      <button type="button" class="btn danger-hollow" onclick={confirmEnd} disabled={busy} aria-busy={busy || undefined}
        >{ending.onAir ? `End the key and take ${who} off air` : 'End the key'}</button
      >
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(34rem, calc(var(--win-w, 100vw) - 2 * var(--s-4)));
    max-height: calc(var(--win-h, 100vh) - 2 * var(--s-4));
    overflow-y: auto;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-3);
    padding: var(--s-5);
  }
  dialog::backdrop {
    background: var(--scrim-strong);
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
  .hk-field {
    display: grid;
    gap: var(--s-1);
  }
  .hk-field > label,
  legend {
    font-weight: 600;
  }
  .opt {
    font-weight: 400;
    color: var(--muted);
    font-size: var(--fs-13);
  }
  .hint {
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
    display: block;
  }
  select,
  input[type='text'],
  textarea {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
    width: 100%;
    min-width: 0;
  }
  textarea {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
    padding: var(--s-2);
    resize: vertical;
  }
  :global(:root[data-theme='light']) select,
  :global(:root[data-theme='light']) input,
  :global(:root[data-theme='light']) textarea {
    color-scheme: light;
  }
  .given {
    display: block;
    font-weight: 600;
    padding: var(--s-1) 0;
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
    margin-bottom: 0;
  }
  .aside,
  .warn {
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    background: var(--surface-2);
    border-left: 3px solid var(--status-warning);
    border-radius: var(--r-1);
    padding: var(--s-2);
    margin-bottom: 0;
  }
  .warn {
    font-size: inherit;
    border-left-color: var(--status-serious);
    margin-bottom: var(--s-3);
  }
  fieldset {
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3) var(--s-3);
    display: grid;
    gap: var(--s-2);
    min-width: 0;
    margin-bottom: var(--s-3);
  }
  form fieldset {
    margin-bottom: 0;
  }
  .way {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: baseline;
    line-height: var(--lh-body);
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3);
    cursor: pointer;
  }
  .way.on {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .way input {
    accent-color: var(--accent);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-2);
  }

  /* The details: the heading and the way out stay in view, and what is between them scrolls,
     so I have sent them is inside the window at any text size. */
  dialog.details {
    width: min(62rem, calc(var(--win-w, 100vw) - 2 * var(--s-4)));
    padding: 0;
    overflow: hidden;
  }
  dialog.details[open] {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .details header {
    padding: var(--s-5) var(--s-5) 0;
  }
  .details header p {
    margin-bottom: var(--s-3);
    max-width: 46rem;
  }
  .details .body {
    overflow-y: auto;
    padding: 0 var(--s-5) var(--s-3);
    display: grid;
    gap: var(--s-3);
    align-content: start;
  }
  .details .acts {
    margin: 0;
    padding: var(--s-3) var(--s-5) var(--s-4);
    border-top: 1px solid var(--hairline);
    background: var(--surface);
  }
  .copy-note {
    margin-right: auto;
    align-self: center;
  }
  .copy-note:empty {
    display: none;
  }
  .ways {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(24rem, 100%), 1fr));
    gap: var(--s-3);
  }
  .waybox {
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-3);
    display: grid;
    gap: var(--s-2);
    align-content: start;
    min-width: 0;
  }
  .waybox:first-child {
    border-color: var(--axis);
  }
  .waybox h3 {
    font-size: var(--fs-16);
    min-height: 1.5rem;
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .waybox p {
    margin: 0;
  }
  .small {
    font-size: var(--fs-13);
    color: var(--ink-2);
    line-height: var(--lh-body);
  }
  .tag {
    font-size: var(--fs-12);
    font-weight: 400;
    color: var(--ink-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-pill);
    padding: 0 var(--s-2);
    line-height: 1.5;
    white-space: nowrap;
  }
  .mon-open {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .f {
    display: grid;
    grid-template-columns: 6.5rem minmax(0, 1fr) auto;
    gap: var(--s-2);
    align-items: center;
  }
  .f .fl {
    font-size: var(--fs-13);
    color: var(--ink-2);
  }
  .f input {
    font-family: var(--font-mono);
    font-size: var(--fs-13);
    min-height: 2rem;
  }
  .f .btn {
    min-width: 4.25rem;
    justify-content: center;
  }
  .f .fw {
    grid-column: 2 / -1;
    font-size: var(--fs-13);
    line-height: var(--lh-body);
    padding: var(--s-1) 0;
  }
  .f .fw.gone {
    color: var(--muted);
  }
  .tell {
    margin: 0;
    line-height: var(--lh-body);
    background: var(--surface-2);
    border-radius: var(--r-2);
    padding: var(--s-3);
    max-width: none;
  }
</style>
