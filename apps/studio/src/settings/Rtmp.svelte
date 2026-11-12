<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > RTMP out (ADR-0230; rtmp-out): where the desk sends the programme's picture and
  // sound, behind the delay (Dump works there too): Rumble, Odysee, PeerTube or any RTMP or
  // RTMPS server. Each destination has its server URL, its stream key (sent once, kept sealed,
  // never shown again), its formats and an on or off. An admin's panel; the station checks
  // everything again and its sentences are shown as given. Panel name `settings.rtmp`.
  import { onDestroy, onMount, untrack } from 'svelte';
  import { addRtmp, changeRtmp, listRtmp, removeRtmp, turnRtmp, type RtmpDestinationInfo, type RtmpList } from '../lib/api.ts';
  import { placeError } from '../lib/account.ts';
  import { PROFILES, keyProblem, nameProblem, profileName, stateOf, urlProblem } from '../lib/rtmp.ts';

  let { initial = null }: { initial?: RtmpList | null } = $props();

  const seed = untrack(() => initial);
  let list = $state<RtmpList | null>(seed);
  let loadError = $state('');
  let note = $state('');
  let working = $state<string | null>(null);
  let rowError = $state<{ id: string; message: string } | null>(null);

  const live = $derived(list?.destinations.filter((d) => d.desk?.state === 'live').length ?? 0);
  const platforms = $derived(list?.platforms ?? []);

  async function refresh() {
    try {
      list = await listRtmp();
      loadError = '';
    } catch (e) {
      loadError = placeError(e, []).message;
    }
  }

  let timer: ReturnType<typeof setInterval> | null = null;
  onMount(() => {
    if (seed === null) void refresh();
    // The desk's view changes as destinations connect and fail: look again every few seconds.
    timer = setInterval(() => void refresh(), 3000);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  // ---- add or change ----

  let formEl = $state<HTMLDialogElement | null>(null);
  let editing = $state<RtmpDestinationInfo | null>(null);
  let draft = $state({ name: '', platform: 'rumble', url: '', key: '', profile: 'compatible', enabled: false });
  let errors = $state<{ name?: string; url?: string; key?: string; form?: string }>({});
  let saving = $state(false);

  const where = $derived(platforms.find((p) => p.platform === draft.platform));

  function askAdd() {
    editing = null;
    const p = platforms[0];
    draft = { name: p?.name ?? '', platform: p?.platform ?? 'rumble', url: '', key: '', profile: 'compatible', enabled: false };
    errors = {};
    formEl?.showModal();
    queueMicrotask(() => document.getElementById('rt-platform')?.focus());
  }

  function askEdit(d: RtmpDestinationInfo) {
    editing = d;
    draft = { name: d.name, platform: d.platform, url: d.url, key: '', profile: d.profile, enabled: d.enabled };
    errors = {};
    formEl?.showModal();
    queueMicrotask(() => document.getElementById('rt-url')?.focus());
  }

  function choosePlatform(p: string) {
    const was = platforms.find((x) => x.platform === draft.platform);
    draft.platform = p;
    const now = platforms.find((x) => x.platform === p);
    // A name that was the old platform's own follows the new one.
    if (!editing && now && (draft.name === '' || draft.name === was?.name)) draft.name = now.name;
  }

  async function onSave(e: SubmitEvent) {
    e.preventDefault();
    if (saving) return;
    const errs: typeof errors = {};
    const n = nameProblem(draft.name);
    const u = urlProblem(draft.url);
    const k = keyProblem(draft.key, editing !== null);
    if (n) errs.name = n;
    if (u) errs.url = u;
    if (k) errs.key = k;
    errors = errs;
    if (n) return document.getElementById('rt-name')?.focus();
    if (u) return document.getElementById('rt-url')?.focus();
    if (k) return document.getElementById('rt-key')?.focus();
    saving = true;
    try {
      const body = {
        name: draft.name.trim(),
        platform: draft.platform,
        url: draft.url.trim(),
        profile: draft.profile,
        enabled: draft.enabled,
        ...(draft.key.trim() ? { key: draft.key.trim() } : {}),
      };
      if (editing) {
        await changeRtmp(editing.id, body);
        note = `${body.name} is changed.`;
      } else {
        await addRtmp({ ...body, key: draft.key.trim() });
        note = `${body.name} is added${body.enabled ? ' and on' : '; turn it on when the show should go there'}. Its key is kept sealed and not shown again.`;
      }
      draft.key = '';
      formEl?.close();
      await refresh();
    } catch (err) {
      const placed = placeError(err, ['name', 'url', 'key', 'platform', 'profile'] as const);
      errors =
        placed.field === 'name' || placed.field === 'url' || placed.field === 'key'
          ? { [placed.field]: placed.message }
          : { form: placed.message };
    } finally {
      saving = false;
    }
  }

  async function turn(d: RtmpDestinationInfo, on: boolean) {
    rowError = null;
    note = '';
    working = d.id;
    try {
      await turnRtmp(d.id, on);
      note = on ? `${d.name} is on: the programme goes there behind the delay.` : `${d.name} is off.`;
      await refresh();
    } catch (e) {
      rowError = { id: d.id, message: placeError(e, []).message };
    } finally {
      working = null;
    }
  }

  // ---- remove (confirmed) ----

  let remEl = $state<HTMLDialogElement | null>(null);
  let removing = $state<RtmpDestinationInfo | null>(null);

  function askRemove(d: RtmpDestinationInfo) {
    removing = d;
    remEl?.showModal();
  }

  async function confirmRemove() {
    const d = removing;
    remEl?.close();
    removing = null;
    if (!d) return;
    working = d.id;
    try {
      await removeRtmp(d.id);
      note = `${d.name} is removed, with its key.`;
      await refresh();
    } catch (e) {
      rowError = { id: d.id, message: placeError(e, []).message };
    } finally {
      working = null;
    }
  }
</script>

<section class="panel" data-panel="settings.rtmp" aria-labelledby="rtmp-h">
  <div class="ph">
    <h2 id="rtmp-h" data-panel-title>RTMP out</h2>
    <span class="small">{#if list}{live} {live === 1 ? 'destination live' : 'destinations live'}{/if}</span>
    <button type="button" class="btn sm add" onclick={askAdd} disabled={!list}>Add a destination…</button>
  </div>
  <p class="help">
    Sends the programme’s picture and sound, behind the delay, to Rumble, Odysee, PeerTube or any RTMP server, so your audience there sees the show without OBS. A dump takes the held words out there too. Each platform shows a server URL and a stream key on its go-live page: paste both here.
  </p>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load RTMP out.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !list}
    <p role="status" class="small">Loading RTMP out…</p>
  {:else}
    {#if !list.video}
      <p class="warn" role="note">The desk runs without video, and RTMP out needs it: turn video on for the station (<span class="mono">VIDEO=1</span>) and restart it.</p>
    {/if}
    {#if list.destinations.length === 0}
      <p class="empty">No destinations yet. Add one with the server URL and stream key from the platform’s go-live page.</p>
    {:else}
      <div class="table-wrap">
        <table>
          <caption class="sr-only">RTMP destinations: name and server, formats, status, and the actions on each</caption>
          <thead>
            <tr><th scope="col">Destination</th><th scope="col">Formats</th><th scope="col">Status</th><th scope="col"><span class="sr-only">Actions</span></th></tr>
          </thead>
          <tbody>
            {#each list.destinations as d (d.id)}
              {@const s = stateOf(d, list.video)}
              {@const busy = working === d.id}
              <tr class:off={!d.enabled} aria-busy={busy || undefined}>
                <th scope="row">
                  {d.name}
                  <span class="small block mono">{d.url}</span>
                </th>
                <td>{profileName(d.profile)}</td>
                <td>
                  <span class="state sm" class:hollow-off={s.tone === 'off'} class:fault={s.tone === 'fault'} class:wait={s.tone === 'wait'}>{s.chip}</span>
                  <span class="small block">{s.line}</span>
                </td>
                <td class="acts">
                  {#if d.enabled}
                    <button type="button" class="btn sm" disabled={busy} onclick={() => turn(d, false)}>Turn off<span class="sr-only">{` ${d.name}`}</span></button>
                  {:else}
                    <button type="button" class="btn sm primary" disabled={busy || !list.video} onclick={() => turn(d, true)}>Turn on<span class="sr-only">{` ${d.name}`}</span></button>
                  {/if}
                  <button type="button" class="btn sm quiet" disabled={busy} onclick={() => askEdit(d)}>Change…<span class="sr-only">{` ${d.name}`}</span></button>
                  <button type="button" class="btn sm quiet" disabled={busy} onclick={() => askRemove(d)}>Remove…<span class="sr-only">{` ${d.name}`}</span></button>
                </td>
              </tr>
              {#if rowError?.id === d.id}
                <tr class="row-err"><td colspan="4"><p class="err" role="alert">{rowError.message}</p></td></tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
    {#if list.encoders.length > 0}
      <p class="small">
        Encoding now:
        {#each list.encoders as e, i (e.profile)}{i > 0 ? '; ' : ''}{profileName(e.profile)} at {e.width}×{e.height}, {e.fps} fps, {e.video_kbps} kbit/s ({e.encoder}){/each}.
      </p>
    {/if}
  {/if}
  <p class="small" role="status" aria-live="polite">{note}</p>
</section>

<dialog data-panel="settings.rtmp.destination" bind:this={formEl} aria-labelledby="rt-h">
  <form onsubmit={onSave} novalidate>
    <h2 id="rt-h">{editing ? `Change ${editing.name}` : 'Add an RTMP destination'}</h2>
    <div class="field">
      <label for="rt-platform">Platform</label>
      <select id="rt-platform" value={draft.platform} onchange={(e) => choosePlatform(e.currentTarget.value)}>
        {#each platforms as p (p.platform)}<option value={p.platform}>{p.name}</option>{/each}
      </select>
      {#if where}<span class="hint">{where.where_to_find}</span>{/if}
    </div>
    <div class="field">
      <label for="rt-name">Name</label>
      <input id="rt-name" type="text" bind:value={draft.name} autocomplete="off" maxlength="40" aria-invalid={errors.name ? true : undefined} aria-describedby={errors.name ? 'rt-name-err' : undefined} />
      {#if errors.name}<p id="rt-name-err" class="err">{errors.name}</p>{/if}
    </div>
    <div class="field">
      <label for="rt-url">Server URL</label>
      <input id="rt-url" type="text" bind:value={draft.url} autocomplete="off" spellcheck="false" placeholder={where?.url || 'rtmps://'} aria-invalid={errors.url ? true : undefined} aria-describedby={errors.url ? 'rt-url-err rt-url-hint' : 'rt-url-hint'} />
      <span id="rt-url-hint" class="hint">As the platform shows it, for example <span class="mono">{where?.url ?? 'rtmp://server/live'}</span>; rtmps:// where the platform offers it.</span>
      {#if errors.url}<p id="rt-url-err" class="err">{errors.url}</p>{/if}
    </div>
    <div class="field">
      <label for="rt-key">Stream key</label>
      <input id="rt-key" type="password" bind:value={draft.key} autocomplete="off" spellcheck="false" aria-invalid={errors.key ? true : undefined} aria-describedby={errors.key ? 'rt-key-err rt-key-hint' : 'rt-key-hint'} />
      <span id="rt-key-hint" class="hint">{editing ? 'Leave it empty to keep the key you gave before.' : 'Kept sealed by the station and never shown again. Anyone with it can stream as you: keep it out of chats and screenshots.'}</span>
      {#if errors.key}<p id="rt-key-err" class="err">{errors.key}</p>{/if}
    </div>
    <fieldset>
      <legend>Formats</legend>
      {#each PROFILES as p (p.profile)}
        <label class="choice">
          <input type="radio" name="rt-profile" value={p.profile} bind:group={draft.profile} />
          <span><strong>{p.name}</strong> <span class="hint">{p.what}</span></span>
        </label>
      {/each}
    </fieldset>
    <label class="choice">
      <input type="checkbox" bind:checked={draft.enabled} />
      <span>On: send the programme there now</span>
    </label>
    {#if errors.form}<p class="err" role="alert">{errors.form}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => formEl?.close()} disabled={saving}>Cancel</button>
      <button type="submit" class="btn primary" disabled={saving} aria-busy={saving || undefined}>{saving ? 'Saving…' : editing ? 'Save' : 'Add the destination'}</button>
    </div>
  </form>
</dialog>

<dialog data-panel="settings.rtmp.remove" bind:this={remEl} aria-labelledby="rtr-h" aria-describedby="rtr-d">
  <h2 id="rtr-h">Remove {removing?.name}?</h2>
  <p id="rtr-d">The desk stops sending there, and the stream key is gone from the station. Add it again with the key to send there later.</p>
  <div class="acts">
    <button type="button" class="btn" onclick={() => remEl?.close()}>Keep it</button>
    <button type="button" class="btn danger-hollow" onclick={confirmRemove}>Remove</button>
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
  .warn {
    border-left: 4px solid var(--status-serious);
    padding-left: var(--s-2);
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
    overflow-wrap: anywhere;
  }
  .state.fault {
    border-color: var(--status-serious);
  }
  .state.wait {
    border-style: dashed;
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
  input[type='text'],
  input[type='password'] {
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
    width: min(36rem, calc(100vw - 2 * var(--s-4)));
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
  .choice {
    display: flex;
    gap: var(--s-2);
    align-items: baseline;
    line-height: var(--lh-body);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-top: var(--s-2);
  }
</style>
