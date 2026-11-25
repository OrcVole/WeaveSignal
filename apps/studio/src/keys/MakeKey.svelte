<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Make a key (keyed shows, 1.3.2): one screen, with the common case already chosen (a personal
  // key for every keyed show, sound and picture, two devices at a time, no end). The key's text
  // comes back once with the answer and goes straight to Send. At its top, the three steps: this
  // is the second, the first (a show kept for people with a key) is a press away and opens over
  // this dialog, so what is typed stays. Panel name `keys.make` (ADR-0070).
  import { untrack } from 'svelte';
  import { placeError } from '../lib/account.ts';
  import { firstProblem, freshForm, keyProblems, specOf, type KeyField, type KeyForm, type KeyProblems } from '../lib/keys.ts';
  import { makeKey, type MadeListenerKey } from '../lib/keysApi.ts';
  import { keyed, ks } from '../lib/keys.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { fromLocalInput } from '../schedule/model.ts';
  import { STEP_WAITS } from '../lib/keyed.ts';
  import KeyFields from './KeyFields.svelte';
  import KeySteps from './KeySteps.svelte';

  let { family = '', onmade, onclose, onkeep }: { family?: string; onmade: (made: MadeListenerKey) => void; onclose: () => void; /** Step 1: Make a show keyed, over this dialog. */ onkeep?: () => void } = $props();

  let el = $state<HTMLDialogElement | null>(null);
  let form = $state<KeyForm>({ ...freshForm(), family: untrack(() => family) });
  let problems = $state<KeyProblems>({});
  let formProblem = $state('');
  let making = $state(false);
  /** After a first try, the fields are checked as they are typed, so a slip put right stops being said. */
  let tried = $state(false);
  $effect(() => {
    if (tried) problems = keyProblems(form, toMs);
  });
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  const toMs = (v: string) => fromLocalInput(v, zone);

  $effect(() => {
    el?.showModal();
    queueMicrotask(() => document.getElementById('mk-label')?.focus());
  });

  const FIELD_OF: Record<string, KeyField> = { label: 'label', email: 'email', note: 'note', shows: 'shows', valid_from_ms: 'from', valid_until_ms: 'until', max_devices: 'devices', max_listens: 'listens', max_people: 'people' };

  function focusOn(f: KeyField | null) {
    if (f) document.getElementById(`mk-${f}`)?.focus();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (making) return;
    formProblem = '';
    tried = true;
    problems = keyProblems(form, toMs);
    const bad = firstProblem(problems);
    if (bad) return focusOn(bad);
    making = true;
    try {
      const made = await makeKey(specOf(form, toMs));
      el?.close();
      onmade(made);
    } catch (err) {
      const placed = placeError(err, Object.keys(FIELD_OF));
      const field = placed.field ? FIELD_OF[placed.field] : null;
      if (field) {
        // The station's own sentence stays under its field until the form is sent again.
        tried = false;
        problems = { [field]: placed.message };
        focusOn(field);
      } else formProblem = placed.message;
    } finally {
      making = false;
    }
  }

  function close() {
    el?.close();
    onclose();
  }
</script>

<dialog bind:this={el} class="kdialog wide" data-panel="keys.make" aria-labelledby="mk-h" oncancel={(e) => { e.preventDefault(); close(); }}>
  <form onsubmit={submit} novalidate>
    <h2 id="mk-h" data-panel-title>Make a key</h2>
    <KeySteps current={2} go={{ 1: onkeep }} waits={{ 3: STEP_WAITS.send }} />
    <p class="lead">A key lets one listener, or one group, hear the shows you keep for key holders. Everything here can be changed later without sending the key again.</p>
    <KeyFields bind:form {problems} shows={keyed.list?.shows ?? []} families={ks.families} mode="make" {zone} now={studio.now || Date.now()} id="mk" two />
    {#if formProblem}<p class="err" role="alert">{formProblem}</p>{/if}
    <div class="acts">
      <button type="button" class="btn quiet" onclick={close} disabled={making}>Cancel</button>
      <button type="submit" class="btn primary" disabled={making} aria-busy={making || undefined}>{making ? 'Making…' : 'Make the key'}</button>
    </div>
  </form>
</dialog>
