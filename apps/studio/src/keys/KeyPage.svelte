<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // One key's own page (keyed shows, 1.3.2): where it stands, what it opens, its window and
  // limits, its use, the devices that hold it with Sign out, and its change log; and everything
  // staff do to it in place, without sending it again: send, change, extend, pause and resume,
  // replace, revoke, and (an admin's) erase. Whatever ends or interrupts someone's listening
  // asks first and says what the holder will see. Panel names `keys.key`, `keys.change` and
  // `keys.ask` (ADR-0070).
  import { placeError } from '../lib/account.ts';
  import { account } from '../lib/account.svelte.ts';
  import { EXTENDS, KIND_WORD, LEAK_WORD, LOST_WAITING, emailFact, lostWaiting, MEDIA_WORD, REASON_MAX, STANDING, actionProblem, alive, changeOf, cutWords, deviceStanding, extendProblem, firstProblem, formOf, fromNow, keyProblems, leakLevel, limitsWords, logWords, mayEraseKey, opensWords, standing, timeWords, windowWords, type KeyField, type KeyForm, type KeyProblems } from '../lib/keys.ts';
  import * as api from '../lib/keysApi.ts';
  import type { KeyLeak, KeyText, ListenerKey } from '../lib/keysApi.ts';
  import { added, closeKey, familyLabel, keyed, ks, openKey, removed, took } from '../lib/keys.svelte.ts';
  import { studio } from '../lib/studio.svelte.ts';
  import { fromLocalInput, localInputValue } from '../schedule/model.ts';
  import KeyFields from './KeyFields.svelte';

  let {
    key,
    onsend,
    leak = null,
  }: {
    key: ListenerKey;
    onsend: (key: ListenerKey, given?: KeyText | null, why?: 'send' | 'made' | 'replaced') => void;
    /** The key's row of the leak report, when the report has been read. */
    leak?: KeyLeak | null;
  } = $props();

  const now = $derived(studio.now || Date.now());
  const zone = $derived(studio.zone || Intl.DateTimeFormat().resolvedOptions().timeZone);
  const toMs = (v: string) => fromLocalInput(v, zone);
  const stand = $derived(standing(key, now, ks.signing?.soonDays));
  const live = $derived(alive(key));
  const shows = $derived(keyed.list?.shows ?? []);
  /** Whether the station sends email, and a Lost your key? request nobody has answered. */
  const mail = $derived(keyed.list?.mail ?? null);
  const waiting = $derived(lostWaiting(ks.log, mail));
  const station = $derived(studio.view?.station || 'the station');
  /** How soon a change reaches someone listening: their player's next renewal, at the station's pace. */
  const soon = $derived(cutWords(ks.signing?.renewS));

  let working = $state('');
  let problem = $state('');

  async function act(name: string, run: () => Promise<ListenerKey>, said: string) {
    if (working) return;
    working = name;
    problem = '';
    try {
      took(await run());
      ks.note = said;
      void openKey(key.id, false);
    } catch (e) {
      problem = actionProblem(e);
    } finally {
      working = '';
    }
  }

  // ---- extend ----
  let extendOpen = $state(false);
  let untilEl = $state<HTMLDialogElement | null>(null);
  let untilValue = $state('');
  let untilErr = $state('');
  function extendBy(days: number, label: string) {
    extendOpen = false;
    void act('extend', () => api.extendKey(key.id, { by_days: days }), `${key.label}’s key is extended (${label}). Nothing needs sending again.`);
  }
  function askUntil() {
    extendOpen = false;
    untilErr = '';
    untilValue = localInputValue(Math.max(key.valid_until_ms ?? now, now) + 7 * 86_400_000, zone);
    untilEl?.showModal();
  }
  async function submitUntil(e: SubmitEvent) {
    e.preventDefault();
    const ms = toMs(untilValue);
    if (ms === null) return void (untilErr = 'Enter a date and a time.');
    if (ms <= now) return void (untilErr = 'The new end is in the future.');
    untilEl?.close();
    await act('extend', () => api.extendKey(key.id, { until_ms: ms }), `${key.label}’s key now ends ${timeWords(ms, zone)}. Nothing needs sending again.`);
  }

  // ---- pause, revoke, replace, erase: each asks first ----
  type Ask = 'pause' | 'revoke' | 'replace' | 'erase';
  let askEl = $state<HTMLDialogElement | null>(null);
  let asking = $state<Ask | null>(null);
  let reason = $state('');
  let askErr = $state('');
  function ask(what: Ask) {
    asking = what;
    reason = '';
    askErr = '';
    askEl?.showModal();
  }
  const ASK: Record<Ask, { title: (k: ListenerKey) => string; body: string; confirm: string; keep: string; reason: string | null; danger: boolean }> = {
    pause: {
      title: (k) => `Pause ${k.label}’s key?`,
      body: 'It is switched off for now and kept: nothing is lost, and Resume brings it back with the same text. Someone listening on it is cut off at once, and their player says the key is switched off for now.',
      confirm: 'Pause the key',
      keep: 'Leave it on',
      reason: 'Why, for the log',
      danger: false,
    },
    revoke: {
      title: (k) => `Revoke ${k.label}’s key?`,
      body: 'It ends for good and cannot be brought back. Someone listening on it is cut off at once, and their player says the key was ended by the station. To give the same person a new key, use Replace instead.',
      confirm: 'Revoke the key',
      keep: 'Keep it',
      reason: 'Why, for the log',
      danger: true,
    },
    replace: {
      title: (k) => `Replace ${k.label}’s key?`,
      body: 'For a key that has leaked, been lost, or is due a change. In one step this key is ended and a new one, with a new text, is made for the same person: the same name, note, email address, shows, window and limits. Every other key is untouched. You then send them the new one.',
      confirm: 'Replace the key',
      keep: 'Keep this one',
      reason: null,
      danger: true,
    },
    erase: {
      title: (k) => `Erase ${k.label}’s key and its records?`,
      body: 'For a person who asks for what the station holds about them to be removed. The key, its name and email address, its change log, its devices and its use records are deleted, and it leaves the leak report. This cannot be undone. To stop a key working, revoke it instead: a revoked key keeps its record.',
      confirm: 'Erase it all',
      keep: 'Keep it',
      reason: null,
      danger: true,
    },
  };
  async function confirmAsk() {
    const what = asking;
    if (!what || working) return;
    if ([...reason].length > REASON_MAX) return void (askErr = `Up to ${REASON_MAX} characters.`);
    working = what;
    askErr = '';
    try {
      if (what === 'pause') took(await api.pauseKey(key.id, reason.trim()));
      else if (what === 'revoke') took(await api.revokeKey(key.id, reason.trim()));
      else if (what === 'erase') {
        const label = key.label;
        await api.eraseKey(key.id);
        askEl?.close();
        asking = null;
        removed(key.id);
        ks.note = `${label}’s key and everything kept about it are erased.`;
        return;
      } else {
        const r = await api.replaceKey(key.id);
        took(r.old);
        askEl?.close();
        asking = null;
        added(r.new);
        ks.note = `${key.label} has a new key. The old one is ended; send them the new one.`;
        onsend(r.new, { text: r.text, link: r.link }, 'replaced');
        return;
      }
      askEl?.close();
      asking = null;
      ks.note = what === 'pause' ? `${key.label}’s key is paused. Resume brings it back.` : `${key.label}’s key is revoked.`;
      void openKey(key.id, false);
    } catch (e) {
      askErr = actionProblem(e);
    } finally {
      working = '';
    }
  }

  // ---- change ----
  let changeEl = $state<HTMLDialogElement | null>(null);
  let form = $state<KeyForm | null>(null);
  let email = $state<'keep' | 'remove' | 'set'>('keep');
  let problems = $state<KeyProblems>({});
  let changeErr = $state('');
  let saving = $state(false);
  let triedChange = $state(false);
  const changeProblems = (f: KeyForm) => {
    const p = keyProblems({ ...f, email: email === 'set' ? f.email : '' }, toMs);
    if (email === 'set' && !f.email.trim()) p.email = 'Enter the new email address.';
    return p;
  };
  $effect(() => {
    if (triedChange && form) problems = changeProblems(form);
  });
  function askChange() {
    triedChange = false;
    form = formOf(key, (ms) => localInputValue(ms, zone));
    email = 'keep';
    problems = {};
    changeErr = '';
    changeEl?.showModal();
  }
  const FIELD_OF: Record<string, KeyField> = { label: 'label', email: 'email', note: 'note', shows: 'shows', valid_from_ms: 'from', valid_until_ms: 'until', max_devices: 'devices', max_listens: 'listens', max_people: 'people' };
  async function submitChange(e: SubmitEvent) {
    e.preventDefault();
    if (!form || saving) return;
    changeErr = '';
    triedChange = true;
    problems = changeProblems(form);
    const bad = firstProblem(problems);
    if (bad) return document.getElementById(`ch-${bad}`)?.focus();
    const change = changeOf(key, form, toMs, email);
    if (Object.keys(change).length === 0) return changeEl?.close();
    saving = true;
    try {
      took(await api.changeKey(key.id, change));
      changeEl?.close();
      ks.note = `${form.label.trim()}’s key is changed. Its text is the same, so nothing needs sending again.`;
      void openKey(key.id, false);
    } catch (err) {
      const placed = placeError(err, Object.keys(FIELD_OF));
      const field = placed.field ? FIELD_OF[placed.field] : null;
      if (field) {
        triedChange = false;
        problems = { [field]: placed.message };
        document.getElementById(`ch-${field}`)?.focus();
      } else changeErr = actionProblem(err);
    } finally {
      saving = false;
    }
  }

  // ---- devices ----
  async function signOut(device: string, label: string) {
    if (working) return;
    working = `dev:${device}`;
    problem = '';
    try {
      await api.endDevice(key.id, device);
      ks.note = `${label} is signed out of ${key.label}’s key. Its place is free at once.`;
      await openKey(key.id, false);
    } catch (e) {
      problem = actionProblem(e);
    } finally {
      working = '';
    }
  }

  const devices = $derived((ks.devices ?? []).map((d) => ({ d, s: deviceStanding(d, now, ks.signing?.holdS != null ? ks.signing.holdS * 1000 : undefined) })));
  const liveCount = $derived(devices.filter((x) => x.s.live).length);
  const replaces = $derived(key.replaces_id);
  const replacedBy = $derived(key.replaced_by_id);
  const extendWhy = $derived(extendProblem(key));
</script>

<aside class="keypage" data-panel="keys.key" aria-label="The key for {key.label}">
  <div class="head">
    <div class="who">
      <h2 data-panel-title>{key.label}</h2>
      <span class="state sm {STANDING[stand].cls}" data-testid="key-standing">{STANDING[stand].word}</span>
    </div>
    <button type="button" class="btn sm quiet" onclick={closeKey} aria-label="Close the key’s page">Close</button>
  </div>
  <p class="standline">{STANDING[stand].line}{#if key.state_reason && key.state_reason !== 'replaced'} <span class="why">“{key.state_reason}”</span>{/if}</p>

  <div class="actions" role="group" aria-label="What to do with this key">
    {#if live}
      <button type="button" class="btn primary" aria-haspopup="dialog" onclick={() => onsend(key)}>Send…</button>
      <button type="button" class="btn" aria-haspopup="dialog" onclick={askChange}>Change…</button>
      <span class="menuwrap">
        <button type="button" class="btn" aria-haspopup="menu" aria-expanded={extendOpen} disabled={!!extendWhy || working === 'extend'} title={extendWhy ?? undefined} onclick={() => (extendOpen = !extendOpen)}>Extend</button>
        {#if extendOpen}
          <div class="kmenu" role="menu" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (extendOpen = false)}>
            {#each EXTENDS as x (x.days)}<button type="button" role="menuitem" onclick={() => extendBy(x.days, x.label)}>{x.label}</button>{/each}
            <button type="button" role="menuitem" onclick={askUntil}>To a date…</button>
          </div>
        {/if}
      </span>
      {#if key.state === 'paused'}
        <button type="button" class="btn" disabled={working === 'resume'} onclick={() => act('resume', () => api.resumeKey(key.id), `${key.label}’s key is on again. Its holder can listen at their next press.`)}>Resume</button>
      {:else}
        <button type="button" class="btn" aria-haspopup="dialog" onclick={() => ask('pause')}>Pause…</button>
      {/if}
      <button type="button" class="btn" aria-haspopup="dialog" onclick={() => ask('replace')}>Replace…</button>
      <button type="button" class="btn danger-hollow" aria-haspopup="dialog" onclick={() => ask('revoke')}>Revoke…</button>
    {:else}
      <p class="small ended">{replacedBy ? 'This key was replaced and is kept for its record.' : 'This key is ended and is kept for its record.'} It cannot be changed.</p>
    {/if}
    {#if mayEraseKey(account.me)}<button type="button" class="btn quiet" aria-haspopup="dialog" onclick={() => ask('erase')}>Erase…</button>{/if}
  </div>
  {#if extendWhy && live}<p class="small">{extendWhy}</p>{/if}
  {#if problem}<p class="err" role="alert">{problem}</p>{/if}
  {#if ks.pageProblem}<p class="err" role="alert">{ks.pageProblem}</p>{/if}

  {#if waiting && live}
    <!-- Lost your key?, at a station that sends no email (1.3.2): the request waits for a person. -->
    <div class="leaknote lostnote" role="status" data-testid="key-lost-waiting">
      <p><b>{LOST_WAITING.lead}</b> {timeWords(waiting.at_ms, zone)}, {LOST_WAITING.body}</p>
      <div class="row"><button type="button" class="btn sm primary" aria-haspopup="dialog" onclick={() => onsend(key)}>Send…</button><span class="small">{LOST_WAITING.hint}</span></div>
    </div>
  {/if}

  {#if leak && leak.score > 0 && live}
    <div class="leaknote" data-testid="key-leak">
      <p><span class="state sm {LEAK_WORD[leakLevel(leak)].cls}">{LEAK_WORD[leakLevel(leak)].word}</span> by the leak report:</p>
      <ul>{#each leak.reasons as r (r)}<li>{r}</li>{/each}</ul>
      <p class="small">A hint, not a verdict. If it has been passed on, <b>Replace…</b> ends this key and gives its holder a new one.</p>
    </div>
  {/if}

  <dl class="facts">
    <dt>Kind</dt>
    <dd>{KIND_WORD[key.kind]}{#if familyLabel(key.family_id)}{' · family '}<b>{familyLabel(key.family_id)}</b>{/if}</dd>
    <dt>Opens</dt>
    <dd>{opensWords(key, shows)} · {MEDIA_WORD[key.media].toLowerCase()}</dd>
    <dt>Works</dt>
    <dd>{windowWords(key, now, zone)}</dd>
    <dt>Limits</dt>
    <dd>{limitsWords(key)}</dd>
    <dt>Email</dt>
    <dd>{emailFact(key.has_email, mail)}</dd>
    {#if key.note}
      <dt>Note</dt>
      <dd>{key.note}</dd>
    {/if}
    <dt>Made</dt>
    <dd>{timeWords(key.created_ms, zone)} by {key.created_by}</dd>
    {#if replaces}
      <dt>Replaces</dt>
      <dd><button type="button" class="linkish" onclick={() => openKey(replaces)}>the key it took the place of</button></dd>
    {/if}
    {#if replacedBy}
      <dt>Replaced by</dt>
      <dd><button type="button" class="linkish" onclick={() => openKey(replacedBy)}>the key that took its place</button></dd>
    {/if}
  </dl>

  <section class="sect" aria-labelledby="kp-use">
    <h3 id="kp-use">Use</h3>
    {#if key.last_used_ms == null}
      <p class="quiet" data-testid="key-unused">Not used yet. Once its holder listens, this says when, how often and on which devices.</p>
    {:else}
      <dl class="facts tight">
        <dt>Listens</dt>
        <dd class="num">{key.listens_used}{key.max_listens != null ? ` of ${key.max_listens}` : ''}</dd>
        <dt>First used</dt>
        <dd>{key.first_used_ms != null ? timeWords(key.first_used_ms, zone) : 'Not known'}</dd>
        <dt>Last used</dt>
        <dd>{timeWords(key.last_used_ms, zone)} <span class="small">({fromNow(key.last_used_ms, now)})</span></dd>
      </dl>
    {/if}
  </section>

  <section class="sect" aria-labelledby="kp-dev">
    <h3 id="kp-dev">Devices {#if ks.devices}<span class="small">{liveCount} listening now{key.kind !== 'machine' && key.max_devices != null ? ` · ${key.max_devices} at once` : ''}</span>{/if}</h3>
    {#if ks.devices === null}
      <p class="quiet" role="status">Reading the devices…</p>
    {:else if devices.length === 0}
      <p class="quiet">No device has used this key.</p>
    {:else}
      <ul class="devices">
        {#each devices as { d, s } (d.id)}
          <li class:over={d.ended_ms != null}>
            <span class="dn"><b>{d.label || 'A browser'}</b><span class="small">{s.word} · first seen {timeWords(d.first_seen_ms, zone)}</span></span>
            {#if s.live}<span class="state sm hollow hollow-good">Now</span>{/if}
            {#if d.ended_ms == null && live}<button type="button" class="btn sm" disabled={!!working} aria-busy={working === `dev:${d.id}` || undefined} onclick={() => signOut(d.id, d.label || 'The device')}>Sign out<span class="sr-only"> {d.label}</span></button>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="sect" aria-labelledby="kp-log">
    <h3 id="kp-log">Change log</h3>
    {#if ks.log === null}
      <p class="quiet" role="status">Reading the log…</p>
    {:else if ks.log.length === 0}
      <p class="quiet">Nothing is logged for this key.</p>
    {:else}
      <ol class="log">
        {#each ks.log as e, i (i)}
          <li><span class="when num">{timeWords(e.at_ms, zone)}</span><span class="what">{logWords(e, zone)}</span><span class="by small">{e.actor}</span></li>
        {/each}
      </ol>
      <p class="small">The log keeps which fields changed, never their values or the key’s text.</p>
    {/if}
  </section>
</aside>

<dialog bind:this={askEl} class="kdialog" data-panel="keys.ask" aria-labelledby="ka-h" aria-describedby="ka-d" oncancel={() => (asking = null)}>
  {#if asking}
    {@const a = ASK[asking]}
    <h2 id="ka-h" data-panel-title>{a.title(key)}</h2>
    <p id="ka-d">{a.body.replace('{soon}', soon)}</p>
    {#if a.reason}
      <div class="field">
        <label for="ka-reason">{a.reason} <span class="opt">optional</span></label>
        <input id="ka-reason" type="text" bind:value={reason} maxlength={REASON_MAX} autocomplete="off" />
        <span class="hint">{asking === 'revoke' ? `Kept with the key. The holder’s player says only that the key was ended by ${station}.` : 'Kept with the key, for whoever looks at it next.'}</span>
      </div>
    {/if}
    {#if askErr}<p class="err" role="alert">{askErr}</p>{/if}
    <div class="acts">
      <button type="button" class="btn" onclick={() => { askEl?.close(); asking = null; }} disabled={!!working}>{a.keep}</button>
      <button type="button" class="btn {a.danger ? 'danger-hollow' : 'primary'}" onclick={confirmAsk} disabled={!!working} aria-busy={!!working || undefined}>{a.confirm}</button>
    </div>
  {/if}
</dialog>

<dialog bind:this={untilEl} class="kdialog narrow" data-panel="keys.extend" aria-labelledby="ku-h">
  <form onsubmit={submitUntil} novalidate>
    <h2 id="ku-h" data-panel-title>Extend {key.label}’s key to a date</h2>
    <div class="field">
      <label for="ku-until">It ends</label>
      <input id="ku-until" type="datetime-local" bind:value={untilValue} aria-invalid={untilErr ? true : undefined} aria-describedby="ku-hint{untilErr ? ' ku-err' : ''}" />
      <span id="ku-hint" class="hint">Station time, {zone.replaceAll('_', ' ')}. {key.valid_until_ms != null ? `Now: ${timeWords(key.valid_until_ms, zone)}.` : ''} The key’s text stays the same.</span>
      {#if untilErr}<p id="ku-err" class="err">{untilErr}</p>{/if}
    </div>
    <div class="acts">
      <button type="button" class="btn quiet" onclick={() => untilEl?.close()}>Cancel</button>
      <button type="submit" class="btn primary">Extend</button>
    </div>
  </form>
</dialog>

<dialog bind:this={changeEl} class="kdialog wide" data-panel="keys.change" aria-labelledby="kc-h">
  {#if form}
    <form onsubmit={submitChange} novalidate>
      <h2 id="kc-h" data-panel-title>Change {key.label}’s key</h2>
      <p class="lead">The key’s text stays the same, so nothing needs sending again. A change reaches someone who is listening {soon}, at their player's next renewal.</p>
      <KeyFields bind:form {problems} {shows} mode="change" {zone} {now} id="ch" hasEmail={key.has_email} bind:email two />
      {#if changeErr}<p class="err" role="alert">{changeErr}</p>{/if}
      <div class="acts">
        <button type="button" class="btn quiet" onclick={() => changeEl?.close()} disabled={saving}>Cancel</button>
        <button type="submit" class="btn primary" disabled={saving} aria-busy={saving || undefined}>{saving ? 'Saving…' : 'Save the changes'}</button>
      </div>
    </form>
  {/if}
</dialog>
