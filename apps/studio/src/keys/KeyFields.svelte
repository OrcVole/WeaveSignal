<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The fields of a listener key (keyed shows, 1.3.2), shared by Make a key, Change and the
  // template of Make many: who it is for, its kind, what it opens, its window in the station's
  // time, its limits and a note. The form holds text, so a half-typed number is not a number
  // yet; lib/keys.ts checks it and makes the body the station takes. The less obvious fields
  // (the kind, what it opens, the window, the limits, a family) carry a hint: a ? with a tooltip.
  import { KIND_LINE, KIND_WORD, LABEL_MAX, MEDIA_WORD, NOTE_MAX, emailHint, type KeyForm, type KeyProblems } from '../lib/keys.ts';
  import { keyed } from '../lib/keys.svelte.ts';
  import type { KeyFamily, KeyKind, KeyMedia, KeyedShow } from '../lib/keysApi.ts';
  import { localInputValue } from '../schedule/model.ts';
  import { KEY_TIPS } from '../lib/keyed.ts';
  import Hint from './Hint.svelte';

  let {
    form = $bindable(),
    problems,
    shows,
    families = [],
    mode,
    zone,
    now,
    id,
    hasEmail = false,
    email = $bindable('keep'),
    two = false,
  }: {
    /** In a wide dialog: the fields in two columns, read down the first and then the second. */
    two?: boolean;
    form: KeyForm;
    problems: KeyProblems;
    /** The station's keyed shows. */
    shows: readonly KeyedShow[];
    families?: readonly KeyFamily[];
    /** `make`: one key; `change`: a key that exists (its kind is fixed); `template`: what every
     *  key of a family is (no name, email or note: those are each recipient's). */
    mode: 'make' | 'change' | 'template';
    zone: string;
    now: number;
    /** A prefix for the fields' ids, so two forms can be on a page. */
    id: string;
    /** In `change`: the station keeps an email address for this key. */
    hasEmail?: boolean;
    /** In `change`: what happens to that address. */
    email?: 'keep' | 'remove' | 'set';
  } = $props();

  const KINDS: readonly KeyKind[] = ['personal', 'group', 'machine'];
  const MEDIA: readonly KeyMedia[] = ['both', 'sound', 'video'];
  const tz = $derived(zone || Intl.DateTimeFormat().resolvedOptions().timeZone);

  function toggleShow(show: string, on: boolean) {
    form.shows = on ? [...form.shows, show] : form.shows.filter((s) => s !== show);
  }
  function endIn(days: number | null) {
    form.until = days === null ? '' : localInputValue(now + days * 86_400_000, tz);
  }
  /** Shows the key names that are not keyed now (or are gone): kept, and said. */
  const strays = $derived(form.shows.filter((s) => !shows.some((x) => x.id === s)));
</script>

<div class="kf" class:two>
  {#if mode !== 'template'}
    <div class="field">
      <label for="{id}-label">Who it is for</label>
      <input id="{id}-label" type="text" bind:value={form.label} autocomplete="off" maxlength={LABEL_MAX} aria-invalid={problems.label ? true : undefined} aria-describedby="{id}-label-hint{problems.label ? ` ${id}-label-err` : ''}" />
      <span id="{id}-label-hint" class="hint">A name, a group or a ticket number: yours, to find the key again. The player never shows it to the key’s holder.</span>
      {#if problems.label}<p id="{id}-label-err" class="err">{problems.label}</p>{/if}
    </div>
  {/if}

  {#if mode !== 'change'}
    <fieldset class="kinds">
      <legend>Kind<Hint id="{id}-kind-tip" about="the kind of key">{KEY_TIPS.kind}</Hint></legend>
      {#each KINDS as kind (kind)}
        <label class="kind" class:on={form.kind === kind}>
          <input type="radio" name="{id}-kind" value={kind} bind:group={form.kind} />
          <span><b>{KIND_WORD[kind]}</b> <span class="hint">{KIND_LINE[kind]}</span></span>
        </label>
      {/each}
    </fieldset>
  {/if}

  {#if mode === 'make'}
    <div class="field">
      <label for="{id}-email">Email address <span class="opt">optional</span></label>
      <input id="{id}-email" type="text" inputmode="email" bind:value={form.email} autocomplete="off" maxlength="254" aria-invalid={problems.email ? true : undefined} aria-describedby="{id}-email-hint{problems.email ? ` ${id}-email-err` : ''}" />
      <span id="{id}-email-hint" class="hint">{emailHint(keyed.list?.mail)}</span>
      {#if problems.email}<p id="{id}-email-err" class="err">{problems.email}</p>{/if}
    </div>
  {:else if mode === 'change'}
    <fieldset class="plain">
      <legend>Email address</legend>
      {#if hasEmail}
        <label class="opt-row"><input type="radio" name="{id}-em" value="keep" bind:group={email} /><span>Keep the address the station has <span class="hint">(it is sealed, and shown only with the key’s text)</span></span></label>
        <label class="opt-row"><input type="radio" name="{id}-em" value="set" bind:group={email} /><span>Use another address</span></label>
        <label class="opt-row"><input type="radio" name="{id}-em" value="remove" bind:group={email} /><span>Remove it <span class="hint">{keyed.list?.mail === false ? '(a Lost your key? request can then no longer be matched to this key)' : '(Lost your key? can then no longer send this key)'}</span></span></label>
      {:else}
        <label class="opt-row"><input type="radio" name="{id}-em" value="keep" bind:group={email} /><span>None kept</span></label>
        <label class="opt-row"><input type="radio" name="{id}-em" value="set" bind:group={email} /><span>Add one</span></label>
      {/if}
      {#if email === 'set'}
        <input id="{id}-email" type="text" inputmode="email" bind:value={form.email} autocomplete="off" maxlength="254" aria-label="The new email address" aria-invalid={problems.email ? true : undefined} aria-describedby={problems.email ? `${id}-email-err` : undefined} />
        {#if problems.email}<p id="{id}-email-err" class="err">{problems.email}</p>{/if}
      {/if}
    </fieldset>
  {/if}

  <fieldset class="plain" id="{id}-shows" tabindex="-1" aria-describedby={problems.shows ? `${id}-shows-err` : undefined}>
    <legend>Opens<Hint id="{id}-opens-tip" about="what the key opens">{KEY_TIPS.opens}</Hint></legend>
    <label class="opt-row"><input type="radio" name="{id}-opens" value="all" bind:group={form.opens} /><span>Every keyed show <span class="hint">now and later</span></span></label>
    <label class="opt-row"><input type="radio" name="{id}-opens" value="some" bind:group={form.opens} disabled={shows.length === 0 && strays.length === 0} /><span>Only these shows</span></label>
    {#if form.opens === 'some'}
      <div class="shows">
        {#each shows as s (s.id)}
          <label class="opt-row"><input type="checkbox" checked={form.shows.includes(s.id)} onchange={(e) => toggleShow(s.id, e.currentTarget.checked)} /><span>{s.name}{#if s.now === 'public'} <span class="hint">open to everyone now</span>{/if}</span></label>
        {/each}
        {#each strays as s (s)}
          <label class="opt-row"><input type="checkbox" checked onchange={(e) => toggleShow(s, e.currentTarget.checked)} /><span>A show that is not keyed now <span class="hint">untick to drop it</span></span></label>
        {/each}
      </div>
    {/if}
    {#if shows.length === 0}<p class="hint">No show needs a key yet, so there is none to choose. A key for every keyed show opens them as they are set.</p>{/if}
    {#if problems.shows}<p id="{id}-shows-err" class="err">{problems.shows}</p>{/if}
  </fieldset>

  <div class="field">
    <span class="labrow"><label for="{id}-media">For</label><Hint id="{id}-media-tip" about="sound and picture">{KEY_TIPS.media}</Hint></span>
    <select id="{id}-media" bind:value={form.media}>
      {#each MEDIA as m (m)}<option value={m}>{MEDIA_WORD[m]}</option>{/each}
    </select>
  </div>

  <fieldset class="plain">
    <legend>Works<Hint id="{id}-works-tip" about="when the key works, and when it ends">{KEY_TIPS.works}</Hint></legend>
    <div class="pair">
      <div class="field">
        <label for="{id}-from">From</label>
        <input id="{id}-from" type="datetime-local" bind:value={form.from} aria-invalid={problems.from ? true : undefined} aria-describedby="{id}-from-hint{problems.from ? ` ${id}-from-err` : ''}" />
        <span id="{id}-from-hint" class="hint">Empty: at once.</span>
        {#if problems.from}<p id="{id}-from-err" class="err">{problems.from}</p>{/if}
      </div>
      <div class="field">
        <label for="{id}-until">Until</label>
        <input id="{id}-until" type="datetime-local" bind:value={form.until} aria-invalid={problems.until ? true : undefined} aria-describedby="{id}-until-hint{problems.until ? ` ${id}-until-err` : ''}" />
        <span id="{id}-until-hint" class="hint">Empty: no end.</span>
        {#if problems.until}<p id="{id}-until-err" class="err">{problems.until}</p>{/if}
      </div>
    </div>
    <div class="quick">
      <span class="hint">End</span>
      <button type="button" class="btn sm quiet" onclick={() => endIn(7)}>in a week</button>
      <button type="button" class="btn sm quiet" onclick={() => endIn(30)}>in a month</button>
      <button type="button" class="btn sm quiet" onclick={() => endIn(365)}>in a year</button>
      <button type="button" class="btn sm quiet" onclick={() => endIn(null)}>never</button>
      <span class="hint tz">Station time, {tz.replaceAll('_', ' ')}</span>
    </div>
  </fieldset>

  <fieldset class="plain">
    <legend>Limits<Hint id="{id}-limits-tip" about="the key’s limits">{KEY_TIPS.limits}</Hint></legend>
    <div class="trio">
      {#if form.kind !== 'machine'}
        <div class="field">
          <label for="{id}-devices">Devices at once</label>
          <input id="{id}-devices" type="text" inputmode="numeric" bind:value={form.devices} placeholder="any" autocomplete="off" aria-invalid={problems.devices ? true : undefined} aria-describedby={problems.devices ? `${id}-devices-err` : undefined} />
          {#if problems.devices}<p id="{id}-devices-err" class="err">{problems.devices}</p>{/if}
        </div>
      {/if}
      <div class="field">
        <label for="{id}-listens">Listens in all</label>
        <input id="{id}-listens" type="text" inputmode="numeric" bind:value={form.listens} placeholder="any" autocomplete="off" aria-invalid={problems.listens ? true : undefined} aria-describedby={problems.listens ? `${id}-listens-err` : undefined} />
        {#if problems.listens}<p id="{id}-listens-err" class="err">{problems.listens}</p>{/if}
      </div>
      {#if form.kind === 'group'}
        <div class="field">
          <label for="{id}-people">People</label>
          <input id="{id}-people" type="text" inputmode="numeric" bind:value={form.people} placeholder="any" autocomplete="off" aria-invalid={problems.people ? true : undefined} aria-describedby={problems.people ? `${id}-people-err` : undefined} />
          {#if problems.people}<p id="{id}-people-err" class="err">{problems.people}</p>{/if}
        </div>
      {/if}
    </div>
    <p class="hint">
      {#if form.kind === 'machine'}A machine key has no devices to count.{:else}A device is a browser or an app; closing it frees its place after a few minutes.{/if}
      A listen starts when a device is new, or comes back after ten minutes. Leave a limit empty for any number.
    </p>
  </fieldset>

  {#if mode !== 'template'}
    <div class="field">
      <label for="{id}-note">Note <span class="opt">optional, for staff</span></label>
      <textarea id="{id}-note" rows="2" bind:value={form.note} maxlength={NOTE_MAX} aria-invalid={problems.note ? true : undefined} aria-describedby={problems.note ? `${id}-note-err` : undefined}></textarea>
      {#if problems.note}<p id="{id}-note-err" class="err">{problems.note}</p>{/if}
    </div>
  {/if}

  {#if mode === 'make' && families.length > 0}
    <div class="field">
      <span class="labrow"><label for="{id}-family">Family <span class="opt">optional</span></label><Hint id="{id}-family-tip" about="families, and tracing a key passed on">{KEY_TIPS.family}</Hint></span>
      <select id="{id}-family" bind:value={form.family}>
        <option value="">None</option>
        {#each families as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
      </select>
      <span class="hint">Keys issued together, one a person, so a key found where it should not be says whose it was.</span>
    </div>
  {/if}
</div>
