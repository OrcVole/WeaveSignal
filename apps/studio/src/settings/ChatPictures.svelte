<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Chat pictures (ADR-0259, ADR-0257, ADR-0262): the switch that turns chatters' uploaded
  // pictures on and off at once (the install setting, AVATAR_UPLOADS, is its default until it is
  // used), and the one for pictures and documents posted in the chat (CHAT_ATTACHMENTS); who may see a picture kept as evidence of suspected child sexual abuse material; and
  // the pictures the station is keeping out of view, each of which staff can delete at once, with
  // why. An evidence picture asks to be confirmed first, in the law's words, and the one named
  // person can look at it and export it for a report. Every change is a line in the moderation
  // record. An admin's panel. Panel names `settings.chat-pictures` and `settings.chat-pictures.delete`
  // (ADR-0070).
  import { onMount, untrack } from 'svelte';
  import { placeError } from '../lib/account.ts';
  import { listStaff, type StaffAccount } from '../lib/api.ts';
  import {
    EVIDENCE_DELETE_WORDS,
    attachmentsWords,
    deleteKept,
    evidenceExport,
    evidencePictureUrl,
    keptKind,
    keptWords,
    kept as loadKept,
    readAttachments,
    readUploads,
    setAttachments,
    setEvidenceViewer,
    setUploads,
    uploadsWords,
    viewerWords,
    type Kept,
    type KeptPicture,
    type UploadsView,
  } from '../lib/chatPictures.ts';

  let {
    initial = null,
    initialAttachments = null,
    initialKept = null,
    initialStaff = null,
    admin = true,
  }: {
    initial?: UploadsView | null;
    initialAttachments?: UploadsView | null;
    initialKept?: Kept | null;
    initialStaff?: StaffAccount[] | null;
    admin?: boolean;
  } = $props();

  const seed = untrack(() => ({ initial, attachments: initialAttachments, kept: initialKept, staff: initialStaff }));
  let view = $state<UploadsView | null>(seed.initial);
  let files = $state<UploadsView | null>(seed.attachments ?? (seed.initial ? { on: false, chosen: false } : null));
  let list = $state<Kept | null>(seed.kept);
  let staff = $state<StaffAccount[]>(seed.staff ?? []);
  let loadError = $state('');
  let error = $state('');
  let busy = $state(false);

  // Deleting one: which, why, and for evidence the confirmation.
  let deleting = $state<KeptPicture | null>(null);
  let why = $state('');
  let confirmed = $state(false);
  let deleteError = $state('');
  // Looking at one (the named person only): the object URL, until closed.
  let looking = $state<{ id: string; url: string } | null>(null);
  let note = $state('');

  // The named person needs the Settings screen to look and export, which is an admin's: offer
  // admins (and whoever is named now, if the API named someone else).
  const people = $derived(
    staff.filter((a) => a.disabled_ms === null && (a.role === 'admin' || a.id === list?.evidence_viewer?.id)),
  );

  async function refresh() {
    try {
      view = await readUploads();
      files = await readAttachments();
      list = await loadKept();
      loadError = '';
    } catch (e) {
      loadError = placeError(e, []).message;
    }
    // The people to choose from need the station's database (a station with no staff accounts
    // has the station key only): without them the picker is left out, not the whole panel.
    if (admin) {
      try {
        staff = await listStaff();
      } catch {
        staff = [];
      }
    }
  }
  onMount(() => {
    if (seed.initial === null) void refresh();
  });

  async function toggle(on: boolean) {
    if (busy || !view || view.on === on) return;
    busy = true;
    error = '';
    try {
      view = await setUploads(on);
    } catch (e) {
      error = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }

  async function toggleFiles(on: boolean) {
    if (busy || !files || files.on === on) return;
    busy = true;
    error = '';
    try {
      files = await setAttachments(on);
    } catch (e) {
      error = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }

  async function chooseViewer(id: string) {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await setEvidenceViewer(id || null);
      list = await loadKept();
    } catch (e) {
      error = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }

  function askDelete(p: KeptPicture) {
    deleting = p;
    why = '';
    confirmed = false;
    deleteError = '';
  }

  async function doDelete() {
    if (!deleting || busy) return;
    if (!why.trim()) {
      deleteError = 'Say why: it goes in the moderation record.';
      return;
    }
    if (deleting.evidence && !confirmed) {
      deleteError = 'Tick the box to confirm.';
      return;
    }
    busy = true;
    deleteError = '';
    try {
      await deleteKept(deleting.id, why, confirmed);
      note = 'Deleted from the server, and recorded.';
      deleting = null;
      list = await loadKept();
    } catch (e) {
      deleteError = placeError(e, []).message;
    } finally {
      busy = false;
    }
  }

  async function look(p: KeptPicture) {
    error = '';
    try {
      if (looking) URL.revokeObjectURL(looking.url);
      looking = { id: p.id, url: await evidencePictureUrl(p.id) };
    } catch (e) {
      error = placeError(e, []).message;
    }
  }
  function closeLook() {
    if (looking) URL.revokeObjectURL(looking.url);
    looking = null;
  }

  async function save(p: KeptPicture) {
    error = '';
    try {
      const { name, blob } = await evidenceExport(p.id);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = name;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
      note = 'The evidence is saved. Do not forward it: it goes to the reporting channel only.';
    } catch (e) {
      error = placeError(e, []).message;
    }
  }
</script>

<section class="panel" data-panel="settings.chat-pictures" aria-labelledby="pics-h">
  <div class="ph">
    <h2 id="pics-h" data-panel-title>Chat pictures</h2>
  </div>

  {#if loadError}
    <div class="callout serious" role="alert">
      <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
      <div>
        <p><strong>Couldn’t load this.</strong> {loadError}</p>
        <button type="button" class="btn sm" onclick={refresh}>Try again</button>
      </div>
    </div>
  {:else if !view}
    <p role="status" class="small">Loading…</p>
  {:else}
    <label class="switch">
      <input
        type="checkbox"
        data-chat="uploads-switch"
        checked={view.on}
        disabled={busy || !admin}
        onchange={(e) => toggle(e.currentTarget.checked)}
      />
      <span>
        <strong>Let chatters upload a picture of their own</strong>
        <span class="hint" data-chat="uploads-words">{uploadsWords(view)}</span>
      </span>
    </label>
    {#if !admin}<p class="small">Ask your station’s admin to change this.</p>{/if}
    <p class="small">
      Turning it off stops new uploads at once, on every open page. The avatars that come with the station stay available.
      The station was installed with <code>AVATAR_UPLOADS</code> as the default until this switch is used.
    </p>

    {#if files}
      <label class="switch">
        <input
          type="checkbox"
          data-chat="attachments-switch"
          checked={files.on}
          disabled={busy || !admin}
          onchange={(e) => toggleFiles(e.currentTarget.checked)}
        />
        <span>
          <strong>Let trusted chatters post pictures and documents</strong>
          <span class="hint" data-chat="attachments-words">{attachmentsWords(files)}</span>
        </span>
      </label>
      <p class="small attach-note" role="note">
        Pictures (PNG, JPEG, WebP) are rewritten at a size that keeps text legible, with nothing but their pixels. Documents (PDF,
        OpenDocument text and spreadsheet, Markdown, plain text) are passed on as they arrived and only ever offered as a download: a PDF
        or an OpenDocument file can carry macros or scripts, and the station does not scan them. <strong
          >Your staff and moderators will see every picture and document before anyone else does.</strong
        >
        Before turning this on, add a line to your risk assessment. The install setting is <code>CHAT_ATTACHMENTS</code>, off.
      </p>
    {/if}

    {#if list}
      <h3>Pictures and documents the station is keeping</h3>
      <p class="small" data-chat="viewer-words">
        {viewerWords(list.evidence_viewer, list.you_are_the_evidence_viewer)}
        A picture refused as suspected child sexual abuse material is not deleted: it is kept for a year, with the upload as it arrived and
        the uploader’s details, for a report to the authorities. See “Handle a picture you suspect is child sexual abuse material”.
      </p>
      {#if admin && people.length > 0}
        <label class="viewer">
          <span>Who sees pictures kept as evidence</span>
          <select
            data-chat="evidence-viewer"
            disabled={busy}
            value={list.evidence_viewer?.id ?? ''}
            onchange={(e) => chooseViewer(e.currentTarget.value)}
          >
            {#each people as a (a.id)}
              <option value={a.id}>{a.name} ({a.username}, {a.role})</option>
            {/each}
          </select>
        </label>
        <p class="small">Until you choose, it is the station’s first admin. Set it, so a named person answers for what is kept.</p>
      {/if}

      {#if list.pictures.length === 0}
        <p class="small" data-chat="kept-empty">None. A refused picture is deleted at once unless it is evidence of something unlawful.</p>
      {:else}
        <ul class="kept" aria-label="Pictures the station is keeping">
          {#each list.pictures as p (p.id)}
            <li data-chat="kept" data-id={p.id} class:evidence={p.evidence}>
              <div>
                <strong>{p.account.name}</strong>
                <span class="hint" data-chat="kept-kind">{keptKind(p)}</span>
                <span class="hint">{keptWords(p)}</span>
                {#if p.evidence}
                  <span class="hint">{p.has_original ? 'With the original upload.' : 'No original: it was approved before it was refused, and an approved upload keeps only its clean copy.'}</span>
                {/if}
              </div>
              <div class="acts">
                {#if p.evidence && list.you_are_the_evidence_viewer}
                  {#if p.kind !== 'document'}
                    <button type="button" class="btn sm" data-chat="evidence-view" onclick={() => look(p)}>View</button>
                  {/if}
                  <button type="button" class="btn sm" data-chat="evidence-export" onclick={() => save(p)}>Export evidence</button>
                {/if}
                <button type="button" class="btn sm danger-hollow" data-chat="kept-delete" onclick={() => askDelete(p)}>Delete now…</button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    {#if looking}
      <div class="look" data-panel="settings.chat-pictures.look" role="dialog" aria-label="A picture kept as evidence">
        <img src={looking.url} alt="Kept as evidence, as the chat shows it" width="256" height="256" />
        <p class="small">Only you can see this. Do not copy, forward or screenshot it.</p>
        <button type="button" class="btn sm" onclick={closeLook}>Close</button>
      </div>
    {/if}

    {#if deleting}
      <div class="del" data-panel="settings.chat-pictures.delete" role="dialog" aria-labelledby="del-h">
        <h3 id="del-h" data-panel-title>Delete {deleting.account.name}’s picture now</h3>
        {#if deleting.evidence}
          <p class="evidence-note" role="note" data-chat="delete-law">{EVIDENCE_DELETE_WORDS}</p>
        {:else}
          <p class="small">It goes from the server, with its files, at once. The line in the moderation record says who deleted it and why.</p>
        {/if}
        <label class="field">
          <span>Why <span class="hint">for the moderation record</span></span>
          <input class="in" data-chat="delete-why" autocomplete="off" bind:value={why} maxlength="500" />
        </label>
        {#if deleting.evidence}
          <label class="check"
            ><input type="checkbox" data-chat="delete-confirm" bind:checked={confirmed} />I have decided to delete it. I know the law asks for a report, then keeping it.</label
          >
        {/if}
        {#if deleteError}<p class="err" role="alert">{deleteError}</p>{/if}
        <div class="acts">
          <button type="button" class="btn sm" onclick={() => (deleting = null)}>Keep it</button>
          <button type="button" class="btn sm primary" data-chat="delete-go" disabled={busy} onclick={doDelete}>Delete now</button>
        </div>
      </div>
    {/if}
    {#if note}<p class="small" role="status" data-chat="note">{note}</p>{/if}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}
</section>

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
  .hint {
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .switch {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: start;
    max-width: 44rem;
  }
  .switch input {
    margin-top: 0.3rem;
  }
  .attach-note {
    max-width: 44rem;
    line-height: var(--lh-body);
  }
  h3 {
    font-size: var(--fs-16);
    margin-top: var(--s-2);
  }
  .viewer {
    display: grid;
    gap: var(--s-1);
    max-width: 30rem;
    font-weight: 600;
  }
  .kept {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-2);
  }
  .kept li {
    display: flex;
    gap: var(--s-3);
    justify-content: space-between;
    align-items: start;
    flex-wrap: wrap;
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-2) var(--s-3);
  }
  .kept li.evidence {
    border-left: 4px solid var(--status-serious);
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    flex-wrap: wrap;
  }
  .del,
  .look {
    border: 1.5px solid var(--axis);
    border-radius: var(--r-2);
    padding: var(--s-3);
    display: grid;
    gap: var(--s-2);
    max-width: 44rem;
    background: var(--surface);
  }
  .evidence-note {
    margin: 0;
    border-left: 4px solid var(--status-serious);
    padding-left: var(--s-2);
    line-height: var(--lh-body);
  }
  .field {
    display: grid;
    gap: var(--s-1);
    font-weight: 600;
  }
  .check {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2);
    align-items: start;
  }
  .look img {
    width: 256px;
    height: 256px;
    object-fit: contain;
    background: var(--surface-2);
    border: 1px solid var(--hairline);
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
    border: 1.5px solid var(--axis);
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
  .callout.serious {
    border-color: var(--status-serious);
  }
</style>
