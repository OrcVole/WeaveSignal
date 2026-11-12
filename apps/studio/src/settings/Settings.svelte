<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings > Retention (ADRs 0113-0116): the Jurisdiction drop-down, each period with the
  // rule it follows and its floor, the licence questions setup must ask, the warnings, and the
  // holds a complaint places. Station admin, reached from the frame; not a numbered tab, so
  // the keymap (ADR-0063) is unchanged, and while it is open no bare key reaches the desk.
  // Panel names `settings.jurisdiction`, `settings.periods`, `settings.holds` (ADR-0070).
  // Your show (ADR-0218) is the MyShow panel's, for a member of staff signed in as themselves.
  // The studio app's updates (ADR-0140) are the Updates panel's, drawn in the packaged app only.
  // Staff accounts (ADR-0127) are the Staff panel's; retention and holds are an admin's, and
  // anyone else is told so here rather than refused by the station.
  import { untrack } from 'svelte';
  import Staff from './Staff.svelte';
  import MyShow from './MyShow.svelte';
  import Zone from './Zone.svelte';
  import Tokens from './Tokens.svelte';
  import Rtmp from './Rtmp.svelte'; // rtmp-out
  import Updates from './Updates.svelte';
  import IcecastOut from './IcecastOut.svelte';
  import ChatPictures from './ChatPictures.svelte';
  import { sandboxed, type Me } from '../lib/api.ts';
  import { account } from '../lib/account.svelte.ts';
  import { isAdmin } from '../lib/account.ts';
  import {
    CUSTOM,
    GROUPS,
    NO_RETENTION,
    choiceOf,
    daysWords,
    goneWords,
    holdKeepUntil,
    holdPastDate,
    localInput,
    periodWords,
    periodsOf,
    preview,
    sameChoice,
    underFloor,
    when,
    type Choice,
    type Hold,
    type Kind,
    type Period,
    type PresetView,
    type RetentionView,
    type Row,
    type SweepReport,
    type Warning,
  } from '../lib/retention.ts';
  import * as api from '../lib/retention.ts';

  let {
    initial = null,
    initialHolds = null,
    me = undefined,
  }: { initial?: RetentionView | null; initialHolds?: Hold[] | null; me?: Me | null } = $props();

  /** Who this window runs as: the prop (a test's), else the station's answer. */
  const who = $derived(me !== undefined ? me : account.me);
  const known = $derived(me !== undefined || account.loaded);
  /** An admin or the key changes retention and holds; an older control plane that cannot say
   *  who the window is counts as the key, which is what it was. */
  const canManage = $derived(!known || who === null || isAdmin(who));
  const showStaff = $derived(!!who && isAdmin(who));
  /** Your show (ADR-0218): for a member of staff signed in as themselves; not in the sandbox. */
  const showMine = $derived(who?.kind === 'staff' && !sandboxed());

  const NAME_KEY = 'weavesignal.settings.name';
  const DAYS_MAX = 36_500;

  // The props seed the screen once (a test's view); after that it is the control plane's.
  const seed = untrack(() => ({ view: initial, holds: initialHolds }));
  let view = $state<RetentionView | null>(seed.view);
  let holdList = $state<Hold[]>(seed.holds ?? []);
  let loadError = $state('');
  let draft = $state<Choice | null>(seed.view ? choiceOf(seed.view) : null);
  /** Day inputs as typed, so an empty or bad entry is the field's error, not a 0. */
  let typed = $state<Partial<Record<Kind, string>>>({});
  let by = $state(readName());
  let pending = $state<{ warnings: Warning[]; sweep: SweepReport } | null>(null);
  let saving = $state(false);
  let saveError = $state('');
  let savedNote = $state('');
  let whoError = $state('');

  function readName(): string {
    try {
      return globalThis.localStorage?.getItem(NAME_KEY) ?? '';
    } catch {
      return '';
    }
  }

  function keepName() {
    try {
      globalThis.localStorage?.setItem(NAME_KEY, by.trim());
    } catch {
      // A private window: the name is this session's only.
    }
  }

  async function refresh() {
    loadError = '';
    try {
      const [v, h] = await Promise.all([api.load(), api.holds()]);
      view = v;
      holdList = h;
      draft = choiceOf(v);
      typed = {};
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  // The first load waits until the station has said who the window is, so a non-admin is
  // told here rather than refused, and the name on the log is the signed-in person's.
  let asked = false;
  $effect(() => {
    if (seed.view || asked || !known || !canManage) return;
    asked = true;
    void refresh();
  });
  // A signed-in member of staff is recorded by their username, whatever is typed (review 0028,
  // C4), so the field shows it and cannot be changed; only the station key names someone.
  const signedIn = $derived(who?.kind === 'staff' ? (who.account?.username ?? null) : null);
  $effect(() => {
    if (signedIn) by = signedIn;
  });

  const presetOf = (key: string | null | undefined): PresetView | null =>
    view?.presets.find((p) => p.key === key) ?? null;
  const saved = $derived(view ? choiceOf(view) : null);
  const dirty = $derived(!!draft && !!saved && !sameChoice(draft, saved));
  /** The preset whose rules and floors apply: the one chosen, or Custom's base. */
  const rules = $derived(presetOf(draft?.preset === CUSTOM ? (draft?.based_on ?? NO_RETENTION) : draft?.preset));
  const chosen = $derived(presetOf(draft?.preset));

  /** The rows as drafted: a preset's own, or Custom's periods with its base's rules. */
  const rows = $derived.by((): Row[] => {
    if (!draft || !view) return [];
    if (draft.preset !== CUSTOM) return presetOf(draft.preset)?.rows ?? view.rows;
    const base = rules?.rows ?? view.rows;
    return base.map((r) => ({ ...r, period: draft!.periods?.[r.kind] ?? r.period }));
  });
  const logger = $derived<Period>(rows.find((r) => r.kind === 'programme_recordings')?.period ?? { keep: 'days', days: 0 });
  const warnings = $derived(dirty ? (pending?.warnings ?? []) : (view?.warnings ?? []));
  const going = $derived(dirty && pending ? pending.sweep.recordings_deleted + pending.sweep.consents_deleted : 0);

  const badDays = $derived(
    Object.entries(typed).filter(([, t]) => {
      const n = Number(t);
      return t === '' || !Number.isInteger(n) || n < 0 || n > DAYS_MAX;
    }).map(([k]) => k as Kind),
  );

  // The draft's warnings and what the next sweep would delete, a moment after each change.
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewSeq = 0;
  $effect(() => {
    const d = draft ? $state.snapshot(draft) : null;
    const isDirty = dirty;
    const bad = badDays.length;
    clearTimeout(previewTimer);
    if (!d || !isDirty || bad) {
      pending = null;
      return;
    }
    const seq = ++previewSeq;
    previewTimer = setTimeout(async () => {
      try {
        const p = await preview(d);
        if (seq === previewSeq) pending = p;
      } catch {
        if (seq === previewSeq) pending = null;
      }
    }, 300);
    return () => clearTimeout(previewTimer);
  });

  function choose(key: string) {
    if (!draft) return;
    savedNote = '';
    if (key === CUSTOM) {
      draft = {
        ...draft,
        preset: CUSTOM,
        based_on: draft.preset === CUSTOM ? draft.based_on : draft.preset,
        periods: periodsOf(rows),
      };
    } else {
      draft = { ...draft, preset: key, based_on: null, periods: null };
      typed = {};
    }
  }

  /** Any period changed turns the drop-down to Custom (ADR-0113). */
  function setPeriod(kind: Kind, p: Period) {
    if (!draft) return;
    savedNote = '';
    const periods = { ...(draft.preset === CUSTOM && draft.periods ? draft.periods : periodsOf(rows)), [kind]: p };
    draft = {
      ...draft,
      preset: CUSTOM,
      based_on: draft.preset === CUSTOM ? draft.based_on : draft.preset,
      periods,
    };
  }

  function typeDays(kind: Kind, text: string, keep: Period['keep']) {
    typed = { ...typed, [kind]: text };
    const n = Number(text);
    const make = (days: number): Period =>
      keep === 'after_recordings' ? { keep: 'after_recordings', days } : { keep: 'days', days };
    if (text !== '' && Number.isInteger(n) && n >= 0 && n <= DAYS_MAX) setPeriod(kind, make(n));
    else if (draft && draft.preset !== CUSTOM) setPeriod(kind, rows.find((r) => r.kind === kind)!.period);
  }

  function daysValue(kind: Kind, p: Period): string {
    return typed[kind] ?? ('days' in p ? String(p.days) : '');
  }

  /** Chat's and consent's way of keeping: a kind of period, then its days where it has them. */
  function setMode(kind: Kind, mode: string) {
    typed = Object.fromEntries(Object.entries(typed).filter(([k]) => k !== kind));
    const p: Period | null =
      mode === 'days'
        ? { keep: 'days', days: 30 }
        : mode === 'after_recordings'
          ? { keep: 'after_recordings', days: 42 }
          : mode === 'with_archive'
            ? { keep: 'with_archive' }
            : mode === 'with_call'
              ? { keep: 'with_call' }
              : null;
    if (p) setPeriod(kind, p);
  }

  function setLicence(field: 'broadcast_licence' | 'us_statutory_licence', on: boolean) {
    if (!draft) return;
    savedNote = '';
    draft = { ...draft, [field]: on };
  }

  function discard() {
    if (!view) return;
    draft = choiceOf(view);
    typed = {};
    saveError = '';
    whoError = '';
  }

  let confirmEl = $state<HTMLDialogElement | null>(null);

  function checkWho(): boolean {
    whoError = by.trim() ? '' : 'Add your name, so the change log says who made this change.';
    if (whoError) document.getElementById('who')?.focus();
    return !whoError;
  }

  function onSave(e: SubmitEvent) {
    e.preventDefault();
    saveError = '';
    if (!dirty || saving) return;
    if (badDays.length) {
      saveError = 'Some periods are not a whole number of days from 0 to 36,500. Correct the marked fields.';
      document.getElementById(`p-${badDays[0]}`)?.focus();
      return;
    }
    if (!checkWho()) return;
    if (going > 0) confirmEl?.showModal();
    else void commit();
  }

  async function commit() {
    if (!draft) return;
    confirmEl?.close();
    saving = true;
    try {
      view = await api.save(draft, by.trim());
      keepName();
      draft = choiceOf(view);
      typed = {};
      savedNote = `Saved. Retention is now ${presetOf(view.setting.preset)?.name ?? view.setting.preset}.`;
    } catch (e) {
      saveError = `Not saved. ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      saving = false;
    }
  }

  // ---- holds ----

  const open = $derived(holdList.filter((h) => h.released_at_ms === null));
  const released = $derived(holdList.filter((h) => h.released_at_ms !== null));
  const nowMs = Date.now();
  let holdFrom = $state(localInput(nowMs - 3_600_000));
  let holdTo = $state(localInput(nowMs));
  let holdReason = $state('');
  let holdErrors = $state<{ from?: string; to?: string; reason?: string; who?: string; form?: string }>({});
  let holdNote = $state('');
  let placing = $state(false);

  async function onPlace(e: SubmitEvent) {
    e.preventDefault();
    holdNote = '';
    const from = Date.parse(holdFrom);
    const to = Date.parse(holdTo);
    const errs: typeof holdErrors = {};
    if (Number.isNaN(from)) errs.from = 'Enter when the broadcast the complaint concerns started.';
    if (Number.isNaN(to)) errs.to = 'Enter when it ended.';
    else if (!errs.from && to <= from) errs.to = 'The end must be after the start.';
    if (!holdReason.trim()) errs.reason = 'Say what the complaint or report is about, and who made it.';
    if (!by.trim()) errs.who = 'Add your name, so the hold log says who placed it.';
    holdErrors = errs;
    const first = (['from', 'to', 'reason', 'who'] as const).find((k) => errs[k]);
    if (first) {
      document.getElementById(`h-${first}`)?.focus();
      return;
    }
    placing = true;
    try {
      const h = await api.placeHold({ reason: holdReason.trim(), by: by.trim(), from_ms: from, to_ms: to });
      keepName();
      holdList = [h, ...holdList];
      holdReason = '';
      holdNote =
        h.recordings || h.consents
          ? `Hold placed. ${keeps(h)} until you release it.`
          : 'Hold placed. Nothing kept now falls in that time; anything recorded then that is added later is held too.';
    } catch (err) {
      holdErrors = { form: `Hold not placed. ${err instanceof Error ? err.message : String(err)}` };
    } finally {
      placing = false;
    }
  }

  let releaseEl = $state<HTMLDialogElement | null>(null);
  let releasing = $state<Hold | null>(null);
  let releaseReason = $state('');
  let releaseErrors = $state<{ reason?: string; who?: string; form?: string }>({});

  function openRelease(h: Hold) {
    releasing = h;
    releaseReason = '';
    releaseErrors = {};
    releaseEl?.showModal();
  }

  async function onRelease(e: SubmitEvent) {
    e.preventDefault();
    if (!releasing) return;
    const errs: typeof releaseErrors = {};
    if (!releaseReason.trim()) errs.reason = 'Say why the hold can end, for example “Complaint closed by Ofcom”.';
    if (!by.trim()) errs.who = 'Add your name, so the hold log says who released it.';
    releaseErrors = errs;
    if (errs.reason || errs.who) {
      document.getElementById(errs.reason ? 'r-reason' : 'r-who')?.focus();
      return;
    }
    try {
      const h = await api.releaseHold(releasing.id, by.trim(), releaseReason.trim());
      keepName();
      holdList = holdList.map((x) => (x.id === h.id ? h : x));
      holdNote = `Hold released. Its records follow their normal period again.`;
      releaseEl?.close();
      releasing = null;
    } catch (err) {
      releaseErrors = { form: `Hold not released. ${err instanceof Error ? err.message : String(err)}` };
    }
  }

  let extendEl = $state<HTMLDialogElement | null>(null);
  let extending = $state<Hold | null>(null);
  let extendUntil = $state('');
  let extendReason = $state('');
  let extendErrors = $state<{ until?: string; reason?: string; who?: string; form?: string }>({});

  function openExtend(h: Hold) {
    extending = h;
    // A month past the date it has now, or past today if that has gone: a date to change.
    const base = Math.max(holdKeepUntil(h, holdRule) ?? 0, Date.now());
    extendUntil = localInput(base + 30 * 86_400_000);
    extendReason = '';
    extendErrors = {};
    extendEl?.showModal();
  }

  async function onExtend(e: SubmitEvent) {
    e.preventDefault();
    if (!extending) return;
    const until = Date.parse(extendUntil);
    const current = holdKeepUntil(extending, holdRule);
    const errs: typeof extendErrors = {};
    if (Number.isNaN(until)) errs.until = 'Enter the date to keep the hold until.';
    else if (until <= Date.now()) errs.until = 'The new date has to be in the future.';
    else if (current !== null && until <= current) errs.until = `The new date has to be later than ${when(current)}, the date it has now.`;
    if (!extendReason.trim()) errs.reason = 'Say why it has to be kept longer, for example “Regulator asked for the tapes again”.';
    if (!by.trim()) errs.who = 'Add your name, so the hold log says who extended it.';
    extendErrors = errs;
    if (errs.until || errs.reason || errs.who) {
      document.getElementById(errs.until ? 'x-until' : errs.reason ? 'x-reason' : 'x-who')?.focus();
      return;
    }
    try {
      const h = await api.extendHold(extending.id, until, by.trim(), extendReason.trim());
      keepName();
      holdList = holdList.map((x) => (x.id === h.id ? h : x));
      holdNote = `Hold extended to ${when(until)}. It is flagged again if it is still open after that.`;
      extendEl?.close();
      extending = null;
    } catch (err) {
      extendErrors = { form: `Hold not extended. ${err instanceof Error ? err.message : String(err)}` };
    }
  }

  function keeps(h: Hold): string {
    if (!h.recordings && !h.consents) return 'Covers no kept records yet';
    const r = `${h.recordings} ${h.recordings === 1 ? 'recording' : 'recordings'}`;
    const c = `${h.consents} consent ${h.consents === 1 ? 'record' : 'records'}`;
    return `Keeps ${r} and ${c}`;
  }

  const holdRule = $derived(rules?.complaint_hold ?? null);
  const glyph = (w: Warning) => (w.level === 'serious' ? { id: 'g-fault', cls: 'g-serious', word: 'Serious' } : { id: 'g-degraded', cls: 'g-warning', word: 'Warning' });
</script>

<main class="settings" aria-labelledby="settings-h">
  <div class="inner">
    <header class="sh">
      <h1 id="settings-h">Settings</h1>
      <p class="lede">Station settings. Changes here apply to the whole station.</p>
    </header>

    <!-- The studio app's own updates: for whoever runs this window, whatever their role. -->
    <Updates />

    {#if showStaff}
      <Staff me={who} />
      <Zone />
      <IcecastOut admin={canManage} />
      <Tokens />
      <Rtmp />
      <!-- Chat pictures (ADR-0259): the chat's own settings, which the sandbox does not simulate. -->
      {#if !sandboxed()}<ChatPictures admin={canManage} />{/if}
    {/if}

    {#if showMine && who}
      <MyShow me={who} />
    {/if}

    {#if !canManage}
      <section class="panel" aria-labelledby="ret-h">
        <div class="ph">
          <h2 id="ret-h">Retention and holds</h2>
        </div>
        <p class="help">
          Only an admin changes how long the station keeps its records, places a hold, or erases someone. Ask the station’s admin.
        </p>
      </section>
    {:else if loadError}
      <div class="callout serious" role="alert">
        <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
        <div>
          <p><strong>Couldn’t load the retention settings.</strong> {loadError}</p>
          <p>Check the control plane is running and has its database, then try again.</p>
          <button type="button" class="btn sm" onclick={refresh}>Try again</button>
        </div>
      </div>
    {:else if !view || !draft}
      <p role="status" class="small">Loading retention settings…</p>
    {:else}
      <form onsubmit={onSave} novalidate aria-labelledby="ret-h">
        <section class="panel" data-panel="settings.jurisdiction" aria-labelledby="ret-h">
          <div class="ph">
            <h2 id="ret-h" data-panel-title>Retention</h2>
            <span class="small">Presets as of {new Date(view.presets_as_of).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })}</span>
          </div>
          <p class="help">
            How long the station keeps what it records. WeaveSignal deletes each record when its period ends, unless a hold keeps it.
          </p>

          <div class="field">
            <label for="jurisdiction" class="lbl">Jurisdiction</label>
            <select
              id="jurisdiction"
              value={draft.preset}
              onchange={(e) => choose(e.currentTarget.value)}
              aria-describedby="jur-summary jur-custom"
            >
              {#each GROUPS as g (g.keys[0])}
                {#if g.label}
                  <optgroup label={g.label}>
                    {#each g.keys as k (k)}<option value={k}>{presetOf(k)?.name ?? k}</option>{/each}
                  </optgroup>
                {:else}
                  {#each g.keys as k (k)}<option value={k}>{presetOf(k)?.name ?? k}{k === NO_RETENTION ? ' (default)' : ''}</option>{/each}
                {/if}
              {/each}
            </select>
            <p id="jur-summary" class="help">
              {#if draft.preset === CUSTOM}
                Every period is set by the station. {rules && rules.key !== NO_RETENTION ? `The floors of ${rules.name} stay as warnings; nothing is blocked.` : ''}
              {:else}
                {chosen?.summary}
              {/if}
            </p>
            <p id="jur-custom" class="help small">Changing any period below switches to Custom.</p>
          </div>

          <fieldset class="licence">
            <legend>About the station</legend>
            <div class="check">
              <input
                id="lic-broadcast"
                type="checkbox"
                checked={draft.broadcast_licence === true}
                onchange={(e) => setLicence('broadcast_licence', e.currentTarget.checked)}
                aria-describedby="lic-broadcast-hint"
              />
              <label for="lic-broadcast">The station holds a broadcast licence</label>
              <p id="lic-broadcast-hint" class="hint">From Ofcom, Coimisiún na Meán, the CRTC, the ACMA or a German state media authority. A licence sets how long recordings must be kept.</p>
            </div>
            <div class="check">
              <input
                id="lic-us"
                type="checkbox"
                checked={draft.us_statutory_licence === true}
                onchange={(e) => setLicence('us_statutory_licence', e.currentTarget.checked)}
                aria-describedby="lic-us-hint"
              />
              <label for="lic-us">The station webcasts music under the US statutory licence</label>
              <p id="lic-us-hint" class="hint">It pays SoundExchange and files Reports of Use, which must be kept 3 years.</p>
            </div>
          </fieldset>

          {#if warnings.length}
            <ul class="warnings" aria-label="Warnings about this setting">
              {#each warnings as w, i (i)}
                {@const g = glyph(w)}
                <li class="callout {w.level}" id={w.kind ? `w-${w.kind}` : undefined}>
                  <svg class={g.cls} aria-hidden="true"><use href="#{g.id}" /></svg>
                  <div>
                    <p><strong><span class="sr-only">{g.word}: </span>{w.title}</strong></p>
                    <p>{w.message}</p>
                    <p class="src">Source: {w.source}</p>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </section>

        <section class="panel" data-panel="settings.periods" aria-labelledby="per-h">
          <div class="ph">
            <h2 id="per-h" data-panel-title>How long each record is kept</h2>
          </div>
          <div class="table-wrap">
            <table>
              <caption class="sr-only">Each kind of record, how long it is kept, and the rule the period follows</caption>
              <thead>
                <tr><th scope="col">Record</th><th scope="col">Kept</th><th scope="col">Rule it follows</th></tr>
              </thead>
              <tbody>
                {#each rows as row (row.kind)}
                  {@const under = underFloor(row, row.period, logger)}
                  {@const bad = badDays.includes(row.kind)}
                  <tr class:under>
                    <th scope="row">
                      {row.label}
                      {#if !row.swept}<span class="small block">WeaveSignal doesn’t keep these yet; the period applies when it does.</span>{/if}
                    </th>
                    <td class="kept">
                      {#if row.kind === 'listener_statistics'}
                        <p>{periodWords(row.period)}</p>
                        <p class="small">Kept under every setting: no IP addresses or names.</p>
                      {:else}
                        {#if row.kind === 'chat_messages' || row.kind === 'consent'}
                          {@const opts =
                            row.kind === 'chat_messages'
                              ? [
                                  { v: 'with_archive', t: 'With the programme’s archive' },
                                  { v: 'days', t: 'For a number of days' },
                                ]
                              : [
                                  { v: 'after_recordings', t: 'While a recording exists, then' },
                                  { v: 'with_call', t: 'Until the call ends' },
                                ]}
                          <label class="sr-only" for="m-{row.kind}">{row.label}: how it is kept</label>
                          <select
                            id="m-{row.kind}"
                            value={row.period.keep}
                            onchange={(e) => setMode(row.kind, e.currentTarget.value)}
                          >
                            {#each opts as o (o.v)}<option value={o.v}>{o.t}</option>{/each}
                          </select>
                        {/if}
                        {#if row.period.keep === 'days' || row.period.keep === 'after_recordings'}
                          <div class="days">
                            <input
                              id="p-{row.kind}"
                              type="number"
                              inputmode="numeric"
                              min="0"
                              max={DAYS_MAX}
                              step="1"
                              value={daysValue(row.kind, row.period)}
                              oninput={(e) => typeDays(row.kind, e.currentTarget.value, row.period.keep)}
                              aria-invalid={bad || undefined}
                              aria-describedby="k-{row.kind}{bad ? ` e-${row.kind}` : ''}{under ? ` w-${row.kind}` : ''}"
                            />
                            <label for="p-{row.kind}">days<span class="sr-only">{` for ${row.label}`}</span></label>
                          </div>
                          {#if bad}<p id="e-{row.kind}" class="err">Enter a whole number of days, 0 to 36,500.</p>{/if}
                        {/if}
                        <p id="k-{row.kind}" class="words">{periodWords(row.period)}</p>
                      {/if}
                    </td>
                    <td class="rule">
                      {#if draft.preset === CUSTOM}
                        <p class="custom-note">Set by the station.</p>
                        <p>{rules && rules.key !== NO_RETENTION ? `${rules.name}: ` : ''}{row.source}</p>
                      {:else}
                        <p>{row.source}</p>
                      {/if}
                      {#if row.floor}
                        <p class="floor">
                          {#if under}
                            <span class="state sm hollow-warning"><svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>Under the floor</span>
                          {/if}
                          The rule requires at least {daysWords(row.floor.days).toLowerCase()}{row.floor.condition ? `, where ${row.floor.condition}` : ''}.
                        </p>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>

        <div class="savebar" class:dirty>
          <p class="impact" role="status" aria-live="polite">
            {#if dirty}
              {#if badDays.length}
                Correct the marked periods to see what saving would delete.
              {:else if !pending}
                Checking what saving would delete…
              {:else if going > 0}
                <strong>Saving deletes {goneWords(pending.sweep)} at the next sweep.</strong>
              {:else}
                Saving deletes nothing now.
              {/if}
            {:else if savedNote}
              {savedNote}
            {:else if view.setting.updated_by && view.setting.updated_at_ms}
              Set by {view.setting.updated_by}, {when(view.setting.updated_at_ms)}.
            {:else}
              Not chosen yet: a new station starts on No retention.
            {/if}
          </p>
          <div class="who">
            <label for="who">Your name</label>
            <input
              id="who"
              type="text"
              autocomplete="name"
              maxlength="120"
              readonly={!!signedIn}
              bind:value={by}
              aria-invalid={whoError ? true : undefined}
              aria-describedby={whoError ? 'who-err who-help' : 'who-help'}
            />
            <span id="who-help" class="hint">{signedIn ? 'You are signed in, so the log names you by your username.' : 'Kept in the log with each change and hold.'}</span>
            {#if whoError}<p id="who-err" class="err">{whoError}</p>{/if}
          </div>
          <div class="acts">
            <button type="button" class="btn quiet" disabled={!dirty || saving} onclick={discard}>Discard changes</button>
            <button type="submit" class="btn primary" disabled={!dirty || saving}>{saving ? 'Saving…' : 'Save changes'}</button>
          </div>
          {#if saveError}<p class="err wide" role="alert">{saveError}</p>{/if}
        </div>
      </form>

      <p class="small sweepline">
        {#if view.last_sweep}
          Last sweep {when(view.last_sweep.finished_at_ms)}: {goneWords(view.last_sweep)} deleted{view.last_sweep.held ? `, ${view.last_sweep.held} kept by holds` : ''}.
        {:else}
          No sweep has run yet.
        {/if}
        {#if view.sweep_every_s}The sweeper runs every {Math.round(view.sweep_every_s / 60)} minutes.{/if}
      </p>

      <section class="panel" data-panel="settings.holds" aria-labelledby="holds-h">
        <div class="ph">
          <h2 id="holds-h" data-panel-title>Holds</h2>
          <span class="small">{open.length} open</span>
        </div>
        <p class="help">
          A hold keeps everything a complaint or report concerns until you release it. After that, the normal period resumes.
        </p>
        {#if holdRule}
          <p class="callout info">
            <svg class="g-accent" aria-hidden="true"><use href="#g-hold" /></svg>
            <span><strong>{rules?.name}:</strong> {holdRule.rule}</span>
          </p>
        {/if}

        <h3>Open holds</h3>
        {#if open.length === 0}
          <p class="empty">No open holds. When a complaint or report arrives, place a hold on the broadcast it concerns, so nothing it needs is deleted.</p>
        {:else}
          <ul class="holds">
            {#each open as h (h.id)}
              <li class="hold">
                <div>
                  <p class="reason">{h.reason}</p>
                  <p class="meta">
                    {#if h.from_ms !== null && h.to_ms !== null}{`Broadcast ${when(h.from_ms)} to ${when(h.to_ms)} · `}{/if}Placed by {h.placed_by}, {when(h.placed_at_ms)} · {keeps(h)}
                  </p>
                  {#if h.extended_until_ms !== null}
                    <p class="meta">Extended to {when(h.extended_until_ms)} by {h.extended_by}, {when(h.extended_at_ms ?? 0)}: {h.extend_reason}</p>
                  {:else if holdRule?.days && h.from_ms !== null}
                    <p class="meta">Keep until at least {when(h.from_ms + holdRule.days * 86_400_000)} if the regulator has notified a complaint.</p>
                  {/if}
                  {#if holdPastDate(h, holdRule, Date.now())}
                    <p class="meta late">
                      <span class="state sm hollow-warning"><svg class="g-warning" aria-hidden="true"><use href="#g-degraded" /></svg>Past its date</span>
                      This hold is still open after its keep-until date. Release it if the complaint is closed, or extend it.
                    </p>
                  {/if}
                </div>
                <div class="hold-acts">
                  <button type="button" class="btn sm" onclick={() => openExtend(h)}>Extend hold…<span class="sr-only">: {h.reason}</span></button>
                  <button type="button" class="btn sm" onclick={() => openRelease(h)}>Release hold…<span class="sr-only">: {h.reason}</span></button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}

        {#if released.length}
          <details class="released">
            <summary>Released holds ({released.length})</summary>
            <p class="hint">A released hold’s record is deleted once the moderation-records period has passed since its release.</p>
            <ul class="holds">
              {#each released as h (h.id)}
                <li class="hold done">
                  <div>
                    <p class="reason">{h.reason}</p>
                    <p class="meta">Placed by {h.placed_by}, {when(h.placed_at_ms)} · Released by {h.released_by}, {when(h.released_at_ms ?? 0)}: {h.release_reason}</p>
                  </div>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <form class="place" onsubmit={onPlace} novalidate>
          <fieldset>
            <legend>Place a hold</legend>
            <p class="hint">Holds every recording and caller’s consent record from the broadcast between these times.</p>
            <div class="grid">
              <div class="field">
                <label for="h-from">From</label>
                <input id="h-from" type="datetime-local" bind:value={holdFrom} aria-invalid={holdErrors.from ? true : undefined} aria-describedby={holdErrors.from ? 'h-from-err' : undefined} />
                {#if holdErrors.from}<p id="h-from-err" class="err">{holdErrors.from}</p>{/if}
              </div>
              <div class="field">
                <label for="h-to">To</label>
                <input id="h-to" type="datetime-local" bind:value={holdTo} aria-invalid={holdErrors.to ? true : undefined} aria-describedby={holdErrors.to ? 'h-to-err' : undefined} />
                {#if holdErrors.to}<p id="h-to-err" class="err">{holdErrors.to}</p>{/if}
              </div>
              <div class="field span">
                <label for="h-reason">Reason</label>
                <input
                  id="h-reason"
                  type="text"
                  maxlength="1000"
                  bind:value={holdReason}
                  aria-invalid={holdErrors.reason ? true : undefined}
                  aria-describedby={holdErrors.reason ? 'h-reason-err h-reason-hint' : 'h-reason-hint'}
                />
                <span id="h-reason-hint" class="hint">What the complaint or report is about, and who made it.</span>
                {#if holdErrors.reason}<p id="h-reason-err" class="err">{holdErrors.reason}</p>{/if}
              </div>
              <div class="field">
                <label for="h-who">Your name</label>
                <input id="h-who" readonly={!!signedIn} type="text" autocomplete="name" maxlength="120" bind:value={by} aria-invalid={holdErrors.who ? true : undefined} aria-describedby={holdErrors.who ? 'h-who-err' : undefined} />
                {#if holdErrors.who}<p id="h-who-err" class="err">{holdErrors.who}</p>{/if}
              </div>
            </div>
            <div class="acts">
              <button type="submit" class="btn primary" disabled={placing}>{placing ? 'Placing…' : 'Place hold'}</button>
            </div>
            {#if holdErrors.form}<p class="err" role="alert">{holdErrors.form}</p>{/if}
            <p class="small" role="status" aria-live="polite">{holdNote}</p>
          </fieldset>
        </form>
      </section>
    {/if}
  </div>

  <dialog data-panel="settings.periods.confirm" bind:this={confirmEl} aria-labelledby="cf-h" aria-describedby="cf-d">
    <h2 id="cf-h">Delete {pending ? goneWords(pending.sweep) : 'records'} at the next sweep?</h2>
    <p id="cf-d">
      The new periods are shorter than what is kept now. At the next sweep, WeaveSignal deletes {pending ? goneWords(pending.sweep) : 'the records'} past them. This can’t be undone. To keep anything a complaint concerns, place a hold first.
    </p>
    <div class="acts">
      <button type="button" class="btn" onclick={() => confirmEl?.close()}>Keep current setting</button>
      <button type="button" class="btn danger-hollow" onclick={commit}>Save and delete</button>
    </div>
  </dialog>

  <dialog data-panel="settings.holds.extend" bind:this={extendEl} aria-labelledby="ex-h" aria-describedby="ex-d">
    <form onsubmit={onExtend} novalidate>
      <h2 id="ex-h">Extend this hold?</h2>
      <p id="ex-d">
        {#if extending}<q>{extending.reason}</q>{/if} The hold stays open and keeps what it keeps until the new date. After that it is flagged again, and nothing releases it but a person.
      </p>
      <div class="field">
        <label for="x-until">Keep it until</label>
        <input id="x-until" type="datetime-local" bind:value={extendUntil} aria-invalid={extendErrors.until ? true : undefined} aria-describedby={extendErrors.until ? 'x-until-err' : undefined} />
        {#if extendErrors.until}<p id="x-until-err" class="err">{extendErrors.until}</p>{/if}
      </div>
      <div class="field">
        <label for="x-reason">Why it has to be kept longer</label>
        <input id="x-reason" type="text" maxlength="1000" bind:value={extendReason} aria-invalid={extendErrors.reason ? true : undefined} aria-describedby={extendErrors.reason ? 'x-reason-err' : undefined} />
        {#if extendErrors.reason}<p id="x-reason-err" class="err">{extendErrors.reason}</p>{/if}
      </div>
      <div class="field">
        <label for="x-who">Your name</label>
        <input id="x-who" readonly={!!signedIn} type="text" autocomplete="name" maxlength="120" bind:value={by} aria-invalid={extendErrors.who ? true : undefined} aria-describedby={extendErrors.who ? 'x-who-err' : undefined} />
        {#if extendErrors.who}<p id="x-who-err" class="err">{extendErrors.who}</p>{/if}
      </div>
      {#if extendErrors.form}<p class="err" role="alert">{extendErrors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn" onclick={() => extendEl?.close()}>Leave it</button>
        <button type="submit" class="btn primary">Extend hold</button>
      </div>
    </form>
  </dialog>

  <dialog data-panel="settings.holds.release" bind:this={releaseEl} aria-labelledby="rl-h" aria-describedby="rl-d">
    <form onsubmit={onRelease} novalidate>
      <h2 id="rl-h">Release this hold?</h2>
      <p id="rl-d">
        {#if releasing}<q>{releasing.reason}</q>{/if} Once released, its records follow their normal period again and may be deleted at the next sweep.
      </p>
      <div class="field">
        <label for="r-reason">Why it can end</label>
        <input id="r-reason" type="text" maxlength="1000" bind:value={releaseReason} aria-invalid={releaseErrors.reason ? true : undefined} aria-describedby={releaseErrors.reason ? 'r-reason-err' : undefined} />
        {#if releaseErrors.reason}<p id="r-reason-err" class="err">{releaseErrors.reason}</p>{/if}
      </div>
      <div class="field">
        <label for="r-who">Your name</label>
        <input id="r-who" type="text" autocomplete="name" maxlength="120" bind:value={by} aria-invalid={releaseErrors.who ? true : undefined} aria-describedby={releaseErrors.who ? 'r-who-err' : undefined} />
        {#if releaseErrors.who}<p id="r-who-err" class="err">{releaseErrors.who}</p>{/if}
      </div>
      {#if releaseErrors.form}<p class="err" role="alert">{releaseErrors.form}</p>{/if}
      <div class="acts">
        <button type="button" class="btn" onclick={() => releaseEl?.close()}>Keep hold</button>
        <button type="submit" class="btn danger-hollow">Release hold</button>
      </div>
    </form>
  </dialog>
</main>

<style>
  .settings {
    height: 100%;
    overflow: auto;
    background: var(--plane);
    /* A field that takes focus scrolls clear of the sticky save bar. */
    scroll-padding-bottom: 9rem;
  }
  .inner {
    max-width: 68rem;
    margin: 0 auto;
    padding: var(--s-5) var(--s-4) var(--s-7);
    display: grid;
    gap: var(--s-4);
  }
  .sh h1 {
    font-size: var(--fs-24);
  }
  .lede {
    color: var(--ink-2);
    margin-top: var(--s-1);
  }
  form {
    display: grid;
    gap: var(--s-4);
  }
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
    justify-content: space-between;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
  .ph h2 {
    font-size: var(--fs-20);
  }
  h3 {
    font-size: var(--fs-16);
    margin-top: var(--s-2);
  }
  .help {
    color: var(--ink-2);
    line-height: var(--lh-body);
    max-width: 60rem;
  }
  .hint {
    display: block;
    font-size: var(--fs-13);
    color: var(--muted);
    line-height: var(--lh-body);
  }
  .small.block {
    display: block;
    font-weight: 400;
    margin-top: var(--s-1);
  }
  .field {
    display: grid;
    gap: var(--s-1);
    align-content: start;
  }
  .lbl,
  .field > label {
    font-weight: 600;
    color: var(--ink);
  }
  select,
  input[type='text'],
  input[type='number'],
  input[type='datetime-local'] {
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
  #jurisdiction {
    max-width: 28rem;
    font-size: var(--fs-16);
  }
  [aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .err {
    color: var(--ink);
    font-size: var(--fs-13);
    border-left: 3px solid var(--status-critical);
    padding-left: var(--s-2);
  }
  fieldset {
    border: 1px solid var(--hairline);
    border-radius: var(--r-2);
    padding: var(--s-3) var(--s-4) var(--s-4);
    margin: 0;
    display: grid;
    gap: var(--s-3);
  }
  legend {
    font-weight: 600;
    padding: 0 var(--s-1);
  }
  .check {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: var(--s-2);
    align-items: center;
  }
  .check input {
    width: 1.125rem;
    height: 1.125rem;
    margin: 0;
    accent-color: var(--accent);
  }
  .check label {
    font-weight: 600;
    cursor: pointer;
  }
  .check .hint {
    grid-column: 2;
  }
  .warnings {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-2);
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
  .callout.warning {
    border-color: var(--status-warning);
  }
  .callout.info {
    border-color: var(--accent);
    margin: 0;
  }
  .callout .src {
    font-size: var(--fs-13);
    color: var(--muted);
    margin-top: var(--s-1);
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
    padding: var(--s-3) var(--s-3) var(--s-3) 0;
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
    width: 22%;
  }
  td.kept {
    width: 30%;
  }
  td.rule {
    color: var(--ink-2);
  }
  tr.under th {
    box-shadow: inset 3px 0 0 var(--status-warning);
    padding-left: var(--s-3);
  }
  .custom-note {
    color: var(--ink);
    font-weight: 600;
  }
  .days {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    margin-top: var(--s-1);
  }
  .days input {
    width: 7rem;
    font-variant-numeric: tabular-nums;
  }
  td.kept select {
    max-width: 100%;
  }
  .words {
    font-size: var(--fs-13);
    color: var(--ink-2);
    margin-top: var(--s-1);
  }
  .floor {
    margin-top: var(--s-2);
    font-size: var(--fs-13);
    color: var(--ink-2);
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
    align-items: center;
  }
  .savebar {
    position: sticky;
    bottom: 0;
    z-index: 2;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--s-3) var(--s-4);
    align-items: end;
    background: var(--surface-2);
    border: 1px solid var(--hairline);
    border-radius: var(--r-3);
    padding: var(--s-3) var(--s-4);
    box-shadow: var(--e-2);
  }
  .savebar.dirty {
    border-color: var(--accent);
  }
  .impact {
    align-self: center;
    line-height: var(--lh-body);
  }
  .who {
    display: grid;
    gap: var(--s-1);
  }
  .who label {
    font-weight: 600;
  }
  .who input {
    width: 14rem;
  }
  .acts {
    display: flex;
    gap: var(--s-2);
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  .err.wide {
    grid-column: 1 / -1;
  }
  .sweepline {
    margin-top: calc(-1 * var(--s-2));
  }
  .empty {
    color: var(--ink-2);
    border: 1px dashed var(--axis);
    border-radius: var(--r-2);
    padding: var(--s-3);
  }
  .holds {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--s-2);
  }
  .hold {
    display: flex;
    justify-content: space-between;
    align-items: start;
    gap: var(--s-3);
    border: 1px solid var(--hairline);
    border-left: 4px solid var(--accent);
    border-radius: var(--r-2);
    padding: var(--s-3);
    background: var(--plane);
  }
  .hold-acts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
    justify-content: flex-end;
  }
  .hold.done {
    border-left-color: var(--axis);
  }
  .hold .reason {
    font-weight: 600;
  }
  .meta {
    font-size: var(--fs-13);
    color: var(--ink-2);
    margin-top: var(--s-1);
  }
  details summary {
    cursor: pointer;
    font-weight: 600;
    min-height: var(--target);
    display: flex;
    align-items: center;
  }
  details[open] summary {
    margin-bottom: var(--s-2);
  }
  .place .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
    gap: var(--s-3);
  }
  .place .span {
    grid-column: 1 / -1;
  }
  dialog {
    max-width: 34rem;
    background: var(--surface);
    color: var(--ink);
    border: 1px solid var(--axis);
    border-radius: var(--r-3);
    box-shadow: var(--e-2);
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
    gap: var(--s-3);
  }
  dialog .acts {
    margin-top: var(--s-3);
  }
  @container (max-width: 720px) {
    .savebar {
      grid-template-columns: 1fr;
    }
    .who input {
      width: 100%;
    }
    tbody th,
    td.kept {
      width: auto;
    }
  }
</style>
