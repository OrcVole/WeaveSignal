<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Make many keys (keyed shows, 1.3.2): a family, one key a recipient, all alike. From a pasted
  // list (a name a line, with an email address and a note if there are any), or a number of keys
  // without names for tickets. Every copy is its own key, so one found where it should not be
  // says whose it was, and one can be replaced while the rest go on working.
  //
  // The answer carries every key's text this once: it is shown, saved or copied from here, and
  // kept nowhere in the app. Panel names `keys.make-many` and `keys.made-many` (ADR-0070).
  import { placeError } from '../lib/account.ts';
  import { saveLabel, saveText, savedWords } from '../lib/saveFile.ts';
  import { FAMILY_MAX, countProblem, csv, familyLabelProblem, familySpecOf, fileStem, firstProblem, freshForm, keyProblems, parseRecipients, templateOf, type KeyForm, type KeyProblems } from '../lib/keys.ts';
  import { makeFamily, type KeyRecipient, type MadeFamily } from '../lib/keysApi.ts';
  import { keyed, ks } from '../lib/keys.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { fromLocalInput } from '../schedule/model.ts';
  import KeyFields from './KeyFields.svelte';
  import KeySteps from './KeySteps.svelte';
  import Hint from './Hint.svelte';
  import { KEY_TIPS } from '../lib/keyed.ts';

  let { onmade, onclose, onkeep }: { onmade: (made: MadeFamily) => void; onclose: () => void; /** Step 1: Make a show keyed, over this dialog. */ onkeep?: () => void } = $props();

  let el = $state<HTMLDialogElement | null>(null);
  let label = $state('');
  let note = $state('');
  let from = $state<'list' | 'count'>('list');
  let pasted = $state('');
  let howMany = $state('');
  let prefix = $state('');
  let form = $state<KeyForm>(freshForm());
  let problems = $state<KeyProblems>({});
  let labelErr = $state('');
  let listErr = $state('');
  let countErr = $state('');
  let formProblem = $state('');
  let making = $state(false);
  let tried = $state(false);
  $effect(() => {
    if (tried) problems = keyProblems(form, toMs, false);
  });
  let made = $state<MadeFamily | null>(null);
  let sent = $state<KeyRecipient[]>([]);
  let copied = $state(false);
  let saved = $state('');
  let saving = $state(false);

  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  const toMs = (v: string) => fromLocalInput(v, zone);
  const parsed = $derived(parseRecipients(pasted));
  const withEmail = $derived(parsed.recipients.filter((r) => r.email).length);
  const n = $derived(from === 'list' ? parsed.recipients.length : /^\d+$/.test(howMany.trim()) ? Number(howMany.trim()) : 0);
  const pre = $derived(prefix.trim() || label.trim() || 'Key');
  const width = $derived(Math.max(3, String(n || 1).length));

  $effect(() => {
    el?.showModal();
    queueMicrotask(() => document.getElementById('mm-label')?.focus());
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (making) return;
    formProblem = '';
    labelErr = familyLabelProblem(label, ks.families.map((f) => f.label)) ?? '';
    listErr = '';
    countErr = '';
    if (from === 'list') {
      if (parsed.problems.length) listErr = parsed.problems.length === 1 ? 'One line cannot be a key. Put it right or take it out.' : `${parsed.problems.length} lines cannot be keys. Put them right or take them out.`;
      else if (parsed.recipients.length === 0) listErr = 'Paste the people, one a line.';
    } else countErr = countProblem(howMany) ?? '';
    tried = true;
    problems = keyProblems(form, toMs, false);
    if (labelErr) return document.getElementById('mm-label')?.focus();
    if (listErr) return document.getElementById('mm-list')?.focus();
    if (countErr) return document.getElementById('mm-count')?.focus();
    const bad = firstProblem(problems);
    if (bad) return document.getElementById(`mm-${bad}`)?.focus();
    making = true;
    sent = from === 'list' ? parsed.recipients : [];
    try {
      const template = templateOf(form, toMs);
      made = await makeFamily(familySpecOf(label, note, template, from === 'list' ? { recipients: parsed.recipients } : { count: n, prefix }));
      onmade(made);
    } catch (err) {
      const placed = placeError(err, ['label', 'recipients', 'count'] as const);
      if (placed.field === 'label') {
        labelErr = placed.message === 'a family has that name already' ? 'A family has that name already. Choose another.' : placed.message;
        document.getElementById('mm-label')?.focus();
      } else if (placed.field === 'recipients') listErr = placed.message;
      else if (placed.field === 'count') countErr = placed.message;
      else formProblem = placed.message;
    } finally {
      making = false;
    }
  }

  /** The keys as the station's own export writes them: for a mail merge or a ticket platform. */
  function asCsv(m: MadeFamily): string {
    return csv(
      ['name', 'email', 'note', 'key', 'link', 'state'],
      m.keys.map((k, i) => [k.key.label, sent[i]?.email ?? '', k.key.note, k.text, k.link, k.key.state]),
    );
  }

  async function copyCsv() {
    if (!made) return;
    try {
      await navigator.clipboard.writeText(asCsv(made));
      copied = true;
    } catch {
      formProblem = 'This window could not copy. Use the family’s Export from the Families list instead.';
    }
  }

  /** Save as CSV: where the person says in the packaged app, a download in a browser. */
  async function saveCsv() {
    if (!made || saving) return;
    saving = true;
    formProblem = '';
    try {
      const r = await saveText(`keys-${fileStem(made.family.label)}.csv`, asCsv(made));
      copied = false;
      saved = savedWords('The list', r);
    } catch (e) {
      formProblem = e instanceof Error ? e.message : 'The file was not saved.';
    } finally {
      saving = false;
    }
  }

  function close() {
    el?.close();
    onclose();
  }
</script>

<dialog bind:this={el} class="kdialog wide" data-panel={made ? 'keys.made-many' : 'keys.make-many'} aria-labelledby="mm-h" oncancel={(e) => { e.preventDefault(); if (!making) close(); }}>
  {#if made}
    <h2 id="mm-h" data-panel-title>{made.keys.length} {made.keys.length === 1 ? 'key' : 'keys'} made in {made.family.label}</h2>
    <KeySteps current={3} go={{ 1: onkeep }} waits={{ 2: 'These keys are made. Done closes this, and Make many… makes more.' }} />
    <p class="lead">Each is its own key. Save them now for a mail merge or a ticket platform: one row a key, with its link.</p>
    <div class="table-wrap short">
      <table>
        <caption class="sr-only">The first keys made: who each is for, its text and its link</caption>
        <thead><tr><th scope="col">For</th><th scope="col">Key</th><th scope="col">Link</th></tr></thead>
        <tbody>
          {#each made.keys.slice(0, 6) as k (k.key.id)}
            <tr><th scope="row">{k.key.label}</th><td class="mono">{k.text}</td><td class="mono clip">{k.link}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if made.keys.length > 6}<p class="small">…and {made.keys.length - 6} more, in the file.</p>{/if}
    <div class="row end">
      <span class="hint grow" role="status">{copied ? 'Copied. Paste it into a spreadsheet or a text file named .csv.' : saved || 'The file holds every key: keep it as you would a list of passwords.'}</span>
      <button type="button" class="btn" onclick={copyCsv}>{copied ? 'Copied' : 'Copy as CSV'}</button>
      <button type="button" class="btn primary" disabled={saving} aria-busy={saving || undefined} onclick={saveCsv}>{saveLabel('Save as CSV')}</button>
    </div>
    {#if formProblem}<p class="err" role="alert">{formProblem}</p>{/if}
    <p class="foot small">Later, the family’s Export gives the same file again, and the station notes on each key that its text went out.</p>
    <div class="acts"><button type="button" class="btn" onclick={close}>Done</button></div>
  {:else}
    <form onsubmit={submit} novalidate>
      <h2 id="mm-h" data-panel-title>Make many keys</h2>
      <KeySteps current={2} go={{ 1: onkeep }} waits={{ 3: 'The keys come back as a list, each with its link, to save as a file and send from your own mail.' }} />
      <p class="lead">A family is one key a person, all alike. If one turns up where it should not, it says whose it was, and it can be replaced while the rest go on working.</p>
      <div class="cols">
        <div class="kf">
          <div class="field">
            <span class="labrow"><label for="mm-label">The family’s name</label><Hint id="mm-family-tip" about="families, and tracing a key passed on">{KEY_TIPS.family}</Hint></span>
            <input id="mm-label" type="text" bind:value={label} oninput={() => (labelErr = '')} autocomplete="off" maxlength="120" aria-invalid={labelErr ? true : undefined} aria-describedby="mm-label-hint{labelErr ? ' mm-label-err' : ''}" />
            <span id="mm-label-hint" class="hint">Who the keys are for: “Autumn patrons”, “Quiz night tickets”.</span>
            {#if labelErr}<p id="mm-label-err" class="err">{labelErr}</p>{/if}
          </div>
          <fieldset class="plain">
            <legend>Who gets a key</legend>
            <label class="opt-row"><input type="radio" name="mm-from" value="list" bind:group={from} /><span>A list of people <span class="hint">pasted from a spreadsheet or typed</span></span></label>
            <label class="opt-row"><input type="radio" name="mm-from" value="count" bind:group={from} /><span>Keys without names <span class="hint">for tickets: numbered, with a word in front</span></span></label>
          </fieldset>
          {#if from === 'list'}
            <div class="field">
              <label for="mm-list">The people, one a line</label>
              <textarea id="mm-list" rows="7" class="mono" bind:value={pasted} oninput={() => (listErr = '')} placeholder={'Isla Fraser, isla@example.org\nCallum Reid, callum@example.org, joined in October\nKatie Ross'} spellcheck="false" aria-invalid={listErr ? true : undefined} aria-describedby="mm-list-hint{listErr ? ' mm-list-err' : ''}"></textarea>
              <span id="mm-list-hint" class="hint">A name, then an email address and a note if you have them, with commas or a spreadsheet’s columns between. A first line of column names is left out.</span>
              <p class="count" role="status" data-testid="mm-count-line">
                {#if parsed.recipients.length}<b>{parsed.recipients.length} {parsed.recipients.length === 1 ? 'person' : 'people'}</b>, {withEmail} with an email address{parsed.header ? ' · the first line was column names' : ''}{:else}No one yet.{/if}
              </p>
              {#if parsed.problems.length}
                <ul class="problems" data-testid="mm-problems">
                  {#each parsed.problems.slice(0, 5) as p (p)}<li>{p}</li>{/each}
                  {#if parsed.problems.length > 5}<li>…and {parsed.problems.length - 5} more.</li>{/if}
                </ul>
              {/if}
              {#if listErr}<p id="mm-list-err" class="err">{listErr}</p>{/if}
            </div>
          {:else}
            <div class="pair">
              <div class="field">
                <label for="mm-count">How many</label>
                <input id="mm-count" type="text" inputmode="numeric" bind:value={howMany} oninput={() => (countErr = '')} autocomplete="off" aria-invalid={countErr ? true : undefined} aria-describedby="mm-count-hint{countErr ? ' mm-count-err' : ''}" />
                <span id="mm-count-hint" class="hint">Up to {FAMILY_MAX.toLocaleString('en-GB')} in one go.</span>
                {#if countErr}<p id="mm-count-err" class="err">{countErr}</p>{/if}
              </div>
              <div class="field">
                <label for="mm-prefix">The word in front <span class="opt">optional</span></label>
                <input id="mm-prefix" type="text" bind:value={prefix} autocomplete="off" maxlength="100" placeholder={label.trim() || 'Ticket'} />
              </div>
            </div>
            <p class="count" role="status">{#if n > 0}They will be named <b>{pre} {String(1).padStart(width, '0')}</b>{n > 1 ? ` to ${pre} ${String(n).padStart(width, '0')}` : ''}.{:else}Each is named with a number: Ticket 001, Ticket 002.{/if}</p>
          {/if}
          <div class="field">
            <label for="mm-note">Note <span class="opt">optional, for staff</span></label>
            <input id="mm-note" type="text" bind:value={note} autocomplete="off" maxlength="500" />
          </div>
        </div>
        <div>
          <p class="caps tmpl">Every key in the family</p>
          <KeyFields bind:form {problems} shows={keyed.list?.shows ?? []} mode="template" {zone} now={studio.now || Date.now()} id="mm" />
        </div>
      </div>
      {#if formProblem}<p class="err" role="alert">{formProblem}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={close} disabled={making}>Cancel</button>
        <button type="submit" class="btn primary" disabled={making} aria-busy={making || undefined}>{making ? 'Making…' : n > 0 ? `Make ${n.toLocaleString('en-GB')} ${n === 1 ? 'key' : 'keys'}` : 'Make the keys'}</button>
      </div>
    </form>
  {/if}
</dialog>
