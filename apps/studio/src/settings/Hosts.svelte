<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Settings › Own studios (ADR-0280, ADR-0284; hosts-in-their-own-studios.md section 4.3): a
  // row for each show key, with who it is for, how they send and how it stands now; Set up a
  // host…, Details…, New key… and End key…; Let back in for a key the desk turned away; and,
  // for an admin, the station's rules for sources. For an admin or a producer; a presenter
  // manages their own show's key from Your show. Panel names `settings.hosts` and
  // `settings.hosts.rules` (ADR-0070); the dialogs are HostKeyFlow's.
  //
  // The station lists keys a show at a time, so the table is one request a show, made together,
  // and a show this window may not read is left out. What is connected now comes from one
  // request (`/sources`), asked again every five seconds while the panel is open. A station
  // older than 1.1 has none of these routes, and the panel is then not drawn at all.
  import { windowChoices } from '../lib/window.ts';
  import { onMount, untrack } from 'svelte';
  import {
    getSourceSettings,
    letSourceBack,
    listShowKeys,
    listSources,
    listStaff,
    putSourceSettings,
    type KeyList,
    type Me,
    type SourceSettings,
    type SourcesNow,
    type StaffAccount,
  } from '../lib/api.ts';
  import * as schedule from '../lib/schedule.ts';
  import { isAdmin, placeError } from '../lib/account.ts';
  import { sched, loadStationFacts } from '../lib/schedule.svelte.ts';
  import { CHIP, type Chip } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import {
    GRACES,
    WAY_WORD,
    hostName,
    hostRows,
    keysSummary,
    nextWords,
    routeAbsent,
    ruleFieldOf,
    rulesProblems,
    showChoices,
    unavailableWords,
    type HostRow,
    type HostShow,
    type KeyChip,
    type RuleField,
  } from '../lib/hosts.ts';
  import HostKeyFlow from './HostKeyFlow.svelte';

  interface Seed {
    shows: HostShow[];
    lists: KeyList[];
    sources: SourcesNow;
    rules?: SourceSettings | null;
    staff?: StaffAccount[];
    zone?: string;
  }
  let { me, initial = null, now: fixedNow = undefined }: { me: Me | null; initial?: Seed | null; now?: number } = $props();

  const seed = untrack(() => initial);
  const admin = $derived(isAdmin(me));
  let shows = $state<HostShow[]>(seed?.shows ?? []);
  let lists = $state<KeyList[]>(seed?.lists ?? []);
  let live = $state<SourcesNow | null>(seed?.sources ?? null);
  let staff = $state<StaffAccount[]>(seed?.staff ?? []);
  let loaded = $state(seed !== null);
  /** The station has no such routes (older than 1.1): nothing is drawn. */
  let absent = $state(false);
  /** The station cannot keep keys: its own sentence. */
  let unavailable = $state('');
  let loadError = $state('');
  let note = $state('');
  let working = $state<string | null>(null);
  let rowError = $state<{ id: string; message: string } | null>(null);
  let now = $state(untrack(() => fixedNow) ?? Date.now());
  const zone = $derived(seed?.zone ?? sched.zone);

  const rows = $derived(hostRows(shows, lists, live?.sources ?? [], now, zone));
  const choices = $derived(showChoices(shows, lists));
  const summary = $derived(keysSummary(rows.live.map((r) => r.status)));
  const listening = $derived(live?.listening ?? true);

  let flow = $state<HostKeyFlow | null>(null);
  /** Whether the host behind a key has their monitor link open; null when they are not connected. */
  const monitorOf = (keyId: string): 'open' | 'closed' | null => {
    const at = live?.sources.find((x) => x.key_id === keyId);
    return at && at.state !== 'not_connected' ? (at.monitor_open ? 'open' : 'closed') : null;
  };

  /** Each show's keys, together; a show this window may not read, or that has gone, is left out. */
  async function readKeys(all: HostShow[]): Promise<KeyList[]> {
    const got = await Promise.allSettled(all.map((s) => listShowKeys(s.slug)));
    const hard = got.find((r) => r.status === 'rejected' && unavailableWords(r.reason));
    if (hard && hard.status === 'rejected') throw hard.reason;
    return got.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : []));
  }

  async function refresh() {
    loadError = '';
    try {
      // The sources first: a station with no such route answers once, not once a show.
      const src = await listSources();
      const [all] = await Promise.all([schedule.shows(), loadStationFacts()]);
      const keys = await readKeys(all);
      live = src;
      shows = all;
      lists = keys;
      unavailable = '';
      loaded = true;
    } catch (e) {
      // Only the first read decides whether the panel is drawn: a later one that fails must not
      // take the dialogs away with a secret still showing.
      if (routeAbsent(e) && !loaded) absent = true;
      else if (unavailableWords(e)) {
        unavailable = unavailableWords(e)!;
        loaded = true;
      } else loadError = placeError(e, []).message;
    }
  }

  /** What is connected now, and the clock the durations are counted from. */
  async function poll() {
    if (!loaded || absent || unavailable) return;
    try {
      const src = await listSources();
      const known = new Set(lists.flatMap((l) => l.keys.map((k) => k.key_id)));
      live = src;
      now = Date.now();
      // A key this window has not read (made in another window) shows up connected: read again.
      if (src.sources.some((s) => !known.has(s.key_id))) void refresh();
    } catch {
      // A missed poll: the row keeps what it last said, and the next poll is five seconds away.
    }
  }

  onMount(() => {
    if (seed !== null) return;
    void refresh();
    void loadRules();
    if (admin) void listStaff().then((a) => (staff = a)).catch(() => {});
    const quick = setInterval(() => void poll(), 5000);
    const slow = setInterval(() => !document.querySelector('dialog[open]') && void refresh(), 60_000);
    return () => {
      clearInterval(quick);
      clearInterval(slow);
    };
  });

  async function letBack(r: HostRow) {
    rowError = null;
    note = '';
    working = r.key.id;
    try {
      await letSourceBack(r.key.key_id);
      note = `${hostName(r.key.label)} is let back in: the key is taken again from now.`;
      await refresh();
    } catch (e) {
      rowError = { id: r.key.id, message: placeError(e, []).message };
    } finally {
      working = null;
    }
  }

  const TEST: Chip = { cls: 'hollow hollow-standby dashed', glyph: 'g-standby', glyphCls: '', word: 'Test' };
  const NOT_CONNECTED: Chip = { cls: 'hollow hollow-standby dashed', glyph: 'g-standby', glyphCls: '', word: 'Not connected' };
  const CHIPS: Record<Exclude<KeyChip, null | 'ended'>, { chip: Chip; title?: string }> = {
    on_air: { chip: CHIP.onAir },
    standby: { chip: CHIP.standby },
    test: { chip: TEST, title: 'A private test, outside the show’s slot: the station hears the host and nobody else does. It never reaches the desk.' },
    not_connected: { chip: NOT_CONNECTED, title: 'The show’s slot is open and its host has not connected.' },
  };

  // ---- the rules (an admin's) ----

  let rules = $state<SourceSettings | null>(seed?.rules ?? null);
  let stored = $state<SourceSettings | null>(seed?.rules ?? null);
  let ruleErrors = $state<Partial<Record<RuleField, string>> & { form?: string }>({});
  let rulesNote = $state('');
  let saving = $state(false);
  const dirty = $derived(!!rules && !!stored && JSON.stringify(rules) !== JSON.stringify(stored));

  async function loadRules() {
    if (!admin) return;
    try {
      const r = await getSourceSettings();
      stored = r;
      rules = { ...r };
    } catch {
      // The table says why when the station cannot keep keys; the rules are then left out.
    }
  }

  const num = (v: string): number => (v.trim() === '' ? NaN : Number(v));

  async function saveRules(e: SubmitEvent) {
    e.preventDefault();
    if (!rules || saving) return;
    const problems = rulesProblems(rules);
    ruleErrors = problems;
    rulesNote = '';
    const first = (Object.keys(problems) as RuleField[])[0];
    if (first) return document.getElementById(`hr-${first}`)?.focus();
    saving = true;
    try {
      const r = await putSourceSettings(rules);
      stored = r;
      rules = { ...r };
      rulesNote = 'The rules are saved.';
    } catch (err) {
      const placed = placeError(err, []);
      const field = ruleFieldOf((err as { field?: string | null } | null)?.field ?? null);
      ruleErrors = field ? { [field]: placed.message } : { form: placed.message };
      if (field) document.getElementById(`hr-${field}`)?.focus();
    } finally {
      saving = false;
    }
  }
</script>

{#if !absent}
  <section class="panel" data-panel="settings.hosts" aria-labelledby="hosts-h">
    <div class="ph">
      <h2 id="hosts-h" data-panel-title>Own studios</h2>
      <span class="small">{#if loaded && !unavailable && rows.live.length}{summary}{/if}</span>
      <button type="button" class="btn sm add" onclick={() => flow?.setup()} disabled={!loaded || !!unavailable || shows.length === 0}>Set up a host…</button>
    </div>
    <p class="help">
      A host can present from a studio of their own. Their software sends to the station with a key that belongs to one show. They arrive standing by, and go on air at their time.
    </p>

    {#if loadError}
      <div class="callout serious" role="alert">
        <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
        <div>
          <p><strong>Couldn’t load the keys.</strong> {loadError}</p>
          <button type="button" class="btn sm" onclick={refresh}>Try again</button>
        </div>
      </div>
    {:else if !loaded}
      <p role="status" class="small">Loading the keys…</p>
    {:else if unavailable}
      <div class="callout serious" role="alert" data-testid="hosts-unavailable">
        <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
        <div><p><strong>The station cannot keep show keys.</strong> {unavailable}</p></div>
      </div>
    {:else}
      {#if !listening}
        <div class="callout serious" role="status" data-testid="hosts-not-listening">
          <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
          <div>
            <p>
              <strong>The station is not listening for hosts.</strong> The part of the station that takes their software is not running, so a host who connects now reaches nothing. Keys can still be made and ended.
            </p>
          </div>
        </div>
      {/if}
      {#if shows.length === 0}
        <p class="empty">The station has no shows yet. A key belongs to a show: make the show in the Schedule tab first.</p>
      {:else if rows.live.length === 0}
        <p class="empty" data-testid="hosts-empty">No host has a key yet. Set one up when a presenter wants to use their own software instead of joining the room.</p>
      {:else}
        <div class="table-wrap">
          <table>
            <caption class="sr-only">Show keys: the show, who the key is for, how they send, how it stands now, and the actions on each</caption>
            <thead>
              <tr><th scope="col">Show</th><th scope="col">Host</th><th scope="col">Way in</th><th scope="col">Status</th><th scope="col"><span class="sr-only">Actions</span></th></tr>
            </thead>
            <tbody>
              {#each rows.live as r (r.key.id)}
                {@const busy = working === r.key.id}
                {@const c = r.status.chip && r.status.chip !== 'ended' ? CHIPS[r.status.chip] : null}
                <tr aria-busy={busy || undefined} data-key={r.key.key_id}>
                  <th scope="row">
                    <span class="show"><i class="sw" style:--sw="var(--cart-{r.show.colour_slot})" aria-hidden="true"></i>{r.showName}</span>
                    <span class="small block">{nextWords(r.show, now, zone)}</span>
                  </th>
                  <td>
                    <span class="who">{r.key.label}</span>
                    <span class="small block">key <span class="mono">{r.key.key_id}</span> · made by {r.key.created_by}</span>
                  </td>
                  <td>{WAY_WORD[r.key.hint] ?? r.key.hint}</td>
                  <td class="status">
                    {#if c}<StateChip chip={c.chip} sm title={c.title} />{/if}
                    {#each r.status.lines as line, i (i)}<span class="small block" class:first={i === 0 && !c}>{line}</span>{/each}
                    {#if r.status.refusal}<span class="small block refusal">{r.status.refusal}</span>{/if}
                    {#if r.status.turnedAway}
                      <span class="small block away">
                        {r.status.turnedAway}
                        <button type="button" class="btn sm" disabled={busy} onclick={() => letBack(r)}>Let back in<span class="sr-only">{` ${r.key.label}`}</span></button>
                      </span>
                    {/if}
                  </td>
                  <td class="acts">
                    <button type="button" class="btn sm quiet" onclick={() => flow?.details(r.key, r.showName, monitorOf(r.key.key_id))}>Details…<span class="sr-only">{` for ${r.key.label}`}</span></button>
                    <button type="button" class="btn sm quiet" disabled={r.key.ends_on_disconnect} title={r.key.ends_on_disconnect ? 'A new key was made already: this one ends when its connection closes.' : undefined} onclick={() => flow?.renew(r.key, r.show, r.status)}
                      >New key…<span class="sr-only">{` for ${r.key.label}`}</span></button
                    >
                    <button type="button" class="btn sm quiet" onclick={() => flow?.end(r.key, r.show, r.status, r.last)}>End key…<span class="sr-only">{` for ${r.key.label}`}</span></button>
                  </td>
                </tr>
                {#if rowError?.id === r.key.id}
                  <tr class="row-err"><td colspan="5"><p class="err" role="alert">{rowError.message}</p></td></tr>
                {/if}
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
      {#if rows.ended.length}
        <details class="ended">
          <summary>Ended keys <span class="small">{rows.ended.length}</span></summary>
          <ul>
            {#each rows.ended as r (r.key.id)}
              <li>
                <span>{r.key.label} · {r.showName} · <span class="mono">{r.key.key_id}</span></span>
                <span class="small">{r.status.lines[0]}{r.status.refusal ? ` · ${r.status.refusal}` : ''}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    {/if}
    <p class="small" role="status" aria-live="polite">{note}</p>

    {#if admin && rules && !unavailable}
      <section class="rules" data-panel="settings.hosts.rules" aria-labelledby="hr-h">
        <h3 id="hr-h" data-panel-title>The rules</h3>
        <form onsubmit={saveRules} novalidate>
          <div class="grid">
            <div class="field">
              <label for="hr-door_before_min">A host may connect before their show</label>
              <span class="unit"
                ><input id="hr-door_before_min" type="number" min="0" max="240" step="1" inputmode="numeric" value={rules.door_before_min} oninput={(e) => (rules!.door_before_min = num(e.currentTarget.value))} aria-invalid={ruleErrors.door_before_min ? true : undefined} aria-describedby="hr-door-hint" />
                minutes</span
              >
              <span id="hr-door-hint" class="hint">From then they are standing by on the desk. Earlier, a connection is a test.</span>
              {#if ruleErrors.door_before_min}<p class="err">{ruleErrors.door_before_min}</p>{/if}
            </div>
            <div class="field">
              <label class="check"><input type="checkbox" checked={rules.test_outside} onchange={(e) => (rules!.test_outside = e.currentTarget.checked)} /> Outside the slot, take a connection as a private test</label>
              <span class="unit"
                >A test lasts at most
                <input id="hr-test_max_min" type="number" min="1" max="60" step="1" inputmode="numeric" aria-label="A test lasts at most, in minutes" value={rules.test_max_min} disabled={!rules.test_outside} oninput={(e) => (rules!.test_max_min = num(e.currentTarget.value))} aria-invalid={ruleErrors.test_max_min ? true : undefined} />
                minutes</span
              >
              <span class="hint">A test never reaches the desk and nobody else hears it. Off, a host outside their slot is refused.</span>
              {#if ruleErrors.test_max_min}<p class="err">{ruleErrors.test_max_min}</p>{/if}
            </div>
            <div class="field">
              <label for="hr-grace_s">At the post, the schedule waits</label>
              <select id="hr-grace_s" value={rules.grace_s} onchange={(e) => (rules!.grace_s = Number(e.currentTarget.value))} aria-describedby="hr-grace-hint">
                {#each GRACES as g (g.s)}<option value={g.s}>{g.label}</option>{/each}
              </select>
              <span id="hr-grace-hint" class="hint">The DJ on air keeps the air until the next is standing by; then the schedule waits this long. Off: the schedule never takes the air from a DJ.</span>
            </div>
            <div class="field">
              <label for="hr-late_count_s">A late host goes on air after</label>
              <span class="unit"
                ><input id="hr-late_count_s" type="number" min="0" max="120" step="1" inputmode="numeric" value={rules.late_count_s} oninput={(e) => (rules!.late_count_s = num(e.currentTarget.value))} aria-invalid={ruleErrors.late_count_s ? true : undefined} aria-describedby="hr-late-hint" />
                seconds</span
              >
              <span id="hr-late-hint" class="hint">With automation holding their air. 0 is off: someone presses.</span>
              {#if ruleErrors.late_count_s}<p class="err">{ruleErrors.late_count_s}</p>{/if}
            </div>
            {#if rules.window_s !== undefined}
              <!-- The handover window (1.2, handover-window.md): absent from a station older than
                   1.2, which is then not drawn and not sent. A length set through the API that is
                   none of the five is shown as it is. -->
              <div class="field">
                <label for="hr-window_s">The desk says who is next</label>
                <select id="hr-window_s" value={rules.window_s} onchange={(e) => (rules!.window_s = Number(e.currentTarget.value))} aria-invalid={ruleErrors.window_s ? true : undefined} aria-describedby="hr-window-hint">
                  {#each windowChoices(stored?.window_s) as w (w.s)}<option value={w.s}>{w.label}</option>{/each}
                </select>
                <span id="hr-window-hint" class="hint">The handover window. From then the desk’s clock names the next host, where they come from and whether they are ready, and the host gets one heads-up. It changes nothing on air. Off: the desk says nothing before the post.</span>
                {#if ruleErrors.window_s}<p class="err">{ruleErrors.window_s}</p>{/if}
              </div>
            {/if}
            <div class="field ports">
              <span class="lab">The ports</span>
              <span class="unit"><span>Icecast, TCP</span> <output id="hr-port_icecast" aria-describedby="hr-ports-hint">{rules.port_icecast || 'Off'}</output></span>
              <span class="unit"><span>SRT, UDP</span> <output id="hr-port_srt" aria-describedby="hr-ports-hint">{rules.port_srt || 'Off'}</output></span>
              <span class="unit"><span>WHIP, UDP</span> <output id="hr-port_whip" aria-describedby="hr-ports-hint">{rules.port_whip || 'Off'}</output></span>
              <span id="hr-ports-hint" class="hint">Set where the station is installed: <code>SOURCE_ICECAST_PORT</code>, <code>SOURCE_SRT_PORT</code> and <code>SOURCE_WHIP_PORT</code> with weavesignal.sh, or the <i>Hosts’ own studios</i> ports in Cloudron’s Location settings. Off until set; the server’s firewall must let the same numbers in.</span>
            </div>
          </div>
          {#if ruleErrors.form}<p class="err" role="alert">{ruleErrors.form}</p>{/if}
          <div class="foot">
            <span class="small" role="status" aria-live="polite">{dirty ? 'Not saved yet.' : rulesNote}</span>
            <button type="submit" class="btn primary" disabled={!dirty || saving} aria-busy={saving || undefined}>{saving ? 'Saving…' : 'Save the rules'}</button>
          </div>
        </form>
      </section>
    {/if}
  </section>

  <HostKeyFlow bind:this={flow} shows={choices} staff={admin ? staff : null} onChanged={refresh} onNote={(n) => (note = n)} />
{/if}

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
    display: block;
  }
  .empty {
    color: var(--ink-2);
    border: 1px dashed var(--axis);
    border-radius: var(--r-2);
    padding: var(--s-3);
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
    white-space: nowrap;
  }
  tbody th {
    font-weight: 600;
  }
  .show {
    display: inline-flex;
    align-items: baseline;
    gap: var(--s-2);
  }
  .sw {
    width: 0.625rem;
    height: 0.625rem;
    border-radius: 2px;
    background: var(--sw);
    flex: none;
    align-self: center;
  }
  .who {
    font-weight: 600;
  }
  .small.block {
    display: block;
    font-weight: 400;
    margin-top: var(--s-1);
  }
  .status .small.block.first {
    margin-top: 0;
    color: var(--ink-2);
  }
  .status :global(.state) {
    vertical-align: top;
  }
  .refusal {
    border-left: 3px solid var(--status-warning);
    padding-left: var(--s-2);
    color: var(--ink);
  }
  .away {
    display: flex !important;
    align-items: center;
    gap: var(--s-2);
    flex-wrap: wrap;
    color: var(--ink);
  }
  td.acts {
    text-align: right;
    white-space: nowrap;
    width: 1%;
    padding-right: 0;
  }
  td.status {
    min-width: 17rem;
  }
  tr.row-err td {
    border-top: 0;
    padding-top: 0;
  }
  .ended summary {
    cursor: pointer;
    color: var(--ink-2);
    font-weight: 600;
  }
  .ended ul {
    list-style: none;
    padding: 0;
    margin: var(--s-2) 0 0;
    display: grid;
    gap: var(--s-1);
    color: var(--muted);
  }
  .ended li {
    display: flex;
    gap: var(--s-3);
    flex-wrap: wrap;
    align-items: baseline;
    line-height: var(--lh-body);
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

  /* the rules */
  .rules {
    border-top: 1px solid var(--hairline);
    padding-top: var(--s-3);
    display: grid;
    gap: var(--s-3);
  }
  .rules h3 {
    font-size: var(--fs-16);
  }
  .rules form {
    display: grid;
    gap: var(--s-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
    gap: var(--s-4) var(--s-5);
    align-items: start;
  }
  .field {
    display: grid;
    gap: var(--s-1);
    align-content: start;
  }
  .field > label,
  .field > .lab {
    font-weight: 600;
    line-height: var(--lh-body);
  }
  .field > label.check {
    display: flex;
    gap: var(--s-2);
    align-items: baseline;
  }
  .unit {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex-wrap: wrap;
    line-height: var(--lh-body);
  }
  .ports .unit > span {
    min-width: 6.5rem;
  }
  .ports output {
    font-family: var(--font-mono);
    color: var(--ink);
  }
  select,
  input[type='number'] {
    font: inherit;
    color: var(--ink);
    background: var(--plane);
    border: 1px solid var(--muted);
    border-radius: var(--r-2);
    min-height: 2.25rem;
    padding: 0 var(--s-2);
    color-scheme: dark;
  }
  input[type='number'] {
    width: 6.5rem;
    font-variant-numeric: tabular-nums;
  }
  select {
    justify-self: start;
    min-width: 10rem;
  }
  :global(:root[data-theme='light']) select,
  :global(:root[data-theme='light']) input {
    color-scheme: light;
  }
  input[type='checkbox'] {
    accent-color: var(--accent);
  }
  [aria-invalid='true'] {
    border-color: var(--status-critical);
    border-width: 2px;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--s-3);
    flex-wrap: wrap;
  }
</style>
