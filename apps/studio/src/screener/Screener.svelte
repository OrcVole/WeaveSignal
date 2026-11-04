<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The screener: the caller queue, the screening conversation (talk to the selected caller,
  // held), send to desk and the desk's answer (DESK BUSY, ADR-0069), drop, a dropped link's held
  // place, BANNED BEFORE (M2); and the chat (M6): its modes with influx as one press with its
  // undo on the same key (ADR-0068), Needs you, the statement of reasons, To the studio and the
  // room, each in its own component.
  import { CHIP, callerChips, masterLamp, LAMP, type Chip } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import * as api from '../lib/api.ts';
  import { run } from '../lib/actions.ts';
  import { say, studio, stationNow, takeView } from '../lib/studio.svelte.ts';
  import { delayWords, dur, firstName, hms, listenerDelayMs, meterPct } from '../lib/format.ts';
  import { onMount } from 'svelte';
  import { chat, startChat } from '../lib/chat.svelte.ts';
  import Needs from './Needs.svelte';
  import StudioLane from './StudioLane.svelte';
  import Statement from './Statement.svelte';
  import Room from './Room.svelte';

  // The chat link starts with the screener and keeps going on the other tabs.
  onMount(startChat);

  const v = $derived(studio.view);
  /** The listener delay: the `studio.listener_delay_ms` setting, labelled until measured (ADR-0103). */
  const LISTENER_DELAY_MS = $derived(listenerDelayMs(v?.listener_delay));
  const behind = $derived(delayWords(v?.listener_delay));
  const now = $derived(stationNow());
  const up = $derived(!!v?.desk.reachable && !!v?.desk.playout.connected);
  const lamp = $derived(
    masterLamp({
      deskReachable: !!v?.desk.reachable,
      playoutConnected: !!v?.desk.playout.connected,
      anyOnAir: (v?.sources ?? []).some((s) => s.on_air && !s.mute),
      mode: v?.mode ?? null,
    }),
  );
  const callers = $derived(v?.callers ?? []);
  const sel = $derived(callers.find((c) => c.identity === studio.selected) ?? null);
  const presenter = $derived(v ? firstName(v.seats.presenter_name, v.seats.presenter ?? 'the presenter') : 'the presenter');
  const talking = $derived(!!sel && studio.talking.includes(sel.identity));
  const talkingToPresenter = $derived(studio.talking.includes('presenter'));
  const who = (c: { name: string; identity: string; name_on_air: string }) => c.name_on_air || firstName(c.name, c.identity);

  // The first caller is selected when nothing is.
  $effect(() => {
    if (!sel && callers.length) studio.selected = callers[0].identity;
  });

  // ---- the screening fields: local while typed, saved after a pause and on leaving ----
  let line = $state('');
  let note = $state('');
  let nameOnAir = $state('');
  let editing = $state(false);
  let loadedFor = '';
  $effect(() => {
    const c = sel;
    if (!c) return;
    if (c.identity !== loadedFor || !editing) {
      loadedFor = c.identity;
      line = c.line;
      note = c.note;
      nameOnAir = c.name_on_air;
    }
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  function save() {
    clearTimeout(timer);
    const c = sel;
    if (!c) return;
    api
      .screener({ op: 'notes', identity: c.identity, line, note, name_on_air: nameOnAir })
      .then(takeView)
      .catch((e) => say('warning', 'Not saved', e instanceof Error ? e.message : String(e)));
  }
  function typed() {
    clearTimeout(timer);
    timer = setTimeout(save, 600);
  }

  // ---- the toast slot under the Room head ----
  type Act = { label: string; keys: string[]; go: () => void; talk?: boolean };
  type Toast = { tone: 'good' | 'warning' | 'serious'; glyph: string; title: string; msg: string; acts: Act[] };
  const dropped = $derived(callers.find((c) => c.dropped_ms !== null) ?? null);
  const declined = $derived(callers.find((c) => c.declined && c.dropped_ms === null) ?? null);
  const lastNotice = $derived(
    v?.notices.filter((n) => n.surface === 'screener' && !studio.dismissed.includes(n.id) && now - n.at_ms < 20_000).at(-1) ?? null,
  );
  const tellPresenter: Act = $derived({ label: `Tell ${presenter} · hold`, keys: ['⌃T'], go: () => {}, talk: true });
  const toast: Toast | null = $derived.by(() => {
    if (studio.note) return { tone: studio.note.tone, glyph: 'g-good', title: studio.note.title, msg: studio.note.message, acts: [] };
    if (v?.chat.influx) {
      return {
        tone: 'warning',
        glyph: 'g-crowd',
        title: `Influx mode on since ${v.chat.influx_since_ms ? hms(v.chat.influx_since_ms) : 'now'}`,
        msg: 'One press, five settings (ADR-0068): accounts only, 24 h old, slow mode 10 s, every new chatter held, links for trusted chatters only. The undo is the same key.',
        acts: [{ label: 'Turn it off', keys: ['G', 'G'], go: () => run({ type: 'influx' }) }, tellPresenter],
      };
    }
    if (dropped) {
      const n = who(dropped);
      return {
        tone: 'warning',
        glyph: 'g-link-degraded',
        title: `${n}'s link dropped at ${hms(dropped.dropped_ms!)}`,
        msg: `Their place in the queue is held for 5 min; if they call in again they come back as number ${dropped.ordinal}. ${presenter} has not been told.`,
        acts: [{ label: 'Let the place go', keys: ['⇧', 'C'], go: () => run({ type: 'release-place' }) }, tellPresenter],
      };
    }
    if (declined) {
      const n = who(declined);
      return {
        tone: 'warning',
        glyph: 'g-desk-busy',
        title: `${presenter} is not taking callers`,
        msg: `${presenter} declined ${n} at ${hms(declined.declined!.at_ms)}${declined.declined!.reason ? `: “${declined.declined!.reason}”` : ''}. ${n} stays ready and hears the programme.`,
        acts: [
          { label: `Send ${n} again`, keys: ['P'], go: () => { studio.selected = declined.identity; void run({ type: 'send' }); } },
          tellPresenter,
        ],
      };
    }
    if (lastNotice) return { tone: lastNotice.tone, glyph: lastNotice.tone === 'good' ? 'g-good' : 'g-degraded', title: lastNotice.title, msg: lastNotice.message, acts: [] };
    return null;
  });

  // The chat's toast (a moderation act, a refused command) sits under the callers' toast and
  // waits while a dropped caller's shows.
  const chatToast = $derived(chat.toast && chat.toast.until > studio.now && !(dropped && !studio.note && !v?.chat.influx) ? chat.toast : null);

  const lampChip: Chip = $derived(LAMP[lamp].chip);
  const influxArmed = $derived(studio.armed?.what === 'influx');
  const dropArmed = $derived(studio.armed?.what === 'drop');
  const waitWord = (c: (typeof callers)[number]) =>
    c.dropped_ms !== null ? 'place held' : c.on_air ? 'on air' : c.cue ? 'cued' : c.state;
</script>

<div class="screener">
  <div class="statusbar" data-panel="screener.status" role="status" aria-label="Station and show" inert={!!chat.sor}>
    <StateChip chip={lampChip} sm />
    <span class="item show" data-panel-title>{v?.show.title || 'No show set'}{v?.seats.presenter ? ` · ${v.seats.presenter_name || v.seats.presenter}` : ''}</span>
    {#if v?.desk_taking_callers ?? true}
      <span class="item"><svg class="g g-good" aria-hidden="true"><use href="#g-good" /></svg>Desk <b>taking callers</b></span>
    {:else}
      <span class="item"><svg class="g g-warning" aria-hidden="true"><use href="#g-desk-busy" /></svg>Desk <b>not taking callers</b></span>
    {/if}
    <span class="item">Listeners <b class="num">—</b></span>
    <span class="sep"></span>
    <span class="item"
      ><svg class="g g-accent" aria-hidden="true"><use href="#g-delay" /></svg><span
        >Studio <b class="num">{hms(now)}</b> · listeners hear <b class="num">{hms(now - LISTENER_DELAY_MS)}</b>, {behind}</span
      ></span
    >
    {#if v?.seats.screener}<span class="item" data-wide><b>{v.seats.screener_name || v.seats.screener}</b> · screener</span>{/if}
    <button
      type="button"
      class="btn sm"
      class:lit-talk={talkingToPresenter}
      aria-label="Talk to {presenter}, held: your voice in {presenter}'s headphones only"
      onpointerdown={() => run({ type: 'talk-seat', held: true })}
      onpointerup={() => run({ type: 'talk-seat', held: false })}
      ><svg class="g" aria-hidden="true"><use href="#g-talk" /></svg>Talk to {presenter} · hold <kbd>⌃T</kbd></button
    >
  </div>

  <main class="app" aria-label="Screener">
    <h1 class="sr-only">{v?.station ?? 'WeaveSignal'} screener: callers and chat</h1>

    <section class="col c1" aria-label="Callers" inert={!!chat.sor}>
      <div class="panel" data-panel="screener.queue">
        <div class="head nowrap">
          <h2>Callers <span class="count" class:attn={callers.length > 0}>{callers.length}</span></h2>
          <span class="sub">from the player page</span>
          <span class="spacer"></span>
        </div>
        <div class="body">
          {#if callers.length}
            <ol class="queue" role="listbox" aria-label="Caller queue">
              {#each callers as c (c.identity)}
                {@const chips = callerChips(c, up, v?.mode ?? null)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <li
                  class="qrow"
                  class:is-onair={c.on_air}
                  class:dropped={c.dropped_ms !== null}
                  role="option"
                  aria-selected={c.identity === studio.selected}
                  tabindex={c.identity === studio.selected ? 0 : -1}
                  onclick={() => (studio.selected = c.identity)}
                >
                  <span class="ord"><kbd>{c.ordinal}</kbd></span>
                  <span class="who"
                    ><span class="name">{c.name || c.identity}</span>
                    <span class="where"
                      >{c.banned_before ? `the account banned ${c.banned_before.date}` : c.identity}{c.dropped_ms !== null
                        ? ` · dropped ${hms(c.dropped_ms)}`
                        : ''}</span
                    ></span
                  >
                  <span class="wait num">{dur(now - (c.dropped_ms ?? c.since_ms))}<small>{waitWord(c)}</small></span>
                  <span class="flags">
                    {#each chips as ch (ch.word)}
                      <StateChip chip={ch} sm focusable={false} hoverable={false} />
                    {/each}
                    {#if c.slot}<span class="tag">strip {c.slot}</span>{/if}
                    {#if !c.consent}<span class="tag no-consent">no consent</span>{/if}
                    {#if c.dropped_ms === null && c.present && c.linked}
                      <span class="link"><svg class="g-good" aria-hidden="true"><use href="#g-link" /></svg>linked</span>
                    {/if}
                    <span class="topic"
                      >{c.banned_before ? `${c.banned_before.reason}, ${c.banned_before.date}.` : ''}
                      {c.dropped_ms !== null ? 'Comes back here, not at the back, within 5 min.' : c.line || c.topic}</span
                    >
                  </span>
                </li>
              {/each}
            </ol>
          {:else}
            <p class="empty">Nobody is calling. Callers join from the player page; they appear here in the order they call.</p>
          {/if}
        </div>
        <div class="qfoot"><kbd>↑</kbd><kbd>↓</kbd> move · <kbd>1</kbd>–<kbd>9</kbd> jump · <kbd>T</kbd> talk · <kbd>P</kbd> to desk · <kbd>⌃D</kbd><kbd>⌃D</kbd> drop</div>
      </div>

      <div class="panel screen" data-panel="screener.screening" aria-label={sel ? `Screening ${who(sel)}` : 'Screening'}>
        <div class="head nowrap">
          {#if sel}
            {@const st = callerChips(sel, up, v?.mode ?? null)[0]}
            <h2 data-panel-title><StateChip chip={st} sm /><span class="nm">{sel.name || sel.identity}</span></h2>
            <span class="sub"><span class="num">{dur(now - sel.since_ms)}</span> · off air</span>
            <span class="spacer"></span>
            <button
              type="button"
              class="btn sm"
              class:lit-talk={talking}
              aria-pressed={talking}
              aria-label="Talk to {who(sel)}, held"
              onpointerdown={() => run({ type: 'screen-talk', held: true })}
              onpointerup={() => run({ type: 'screen-talk', held: false })}
              onpointerleave={() => talking && run({ type: 'screen-talk', held: false })}>Talk · hold <kbd>T</kbd></button
            >
          {:else}
            <h2 data-panel-title>Screening</h2>
          {/if}
        </div>
        <div class="body">
          {#if sel}
            <div class="form">
              <div class="field">
                <label for="f-line">One line for {presenter}</label>
                <input class="in" id="f-line" bind:value={line} oninput={typed} onfocus={() => (editing = true)} onblur={() => { editing = false; save(); }} placeholder={sel.topic || 'What they want to say, in a line'} />
              </div>
              <div class="chks" aria-label="Checks">
                {#if sel.consent}
                  <span class="chk-box" data-consent="given"
                    ><i class="on"><svg aria-hidden="true"><use href="#g-check" /></svg></i>Consent to record · {sel.consent.by === 'caller'
                      ? `accepted ${hms(sel.consent.at_ms)} on the player page`
                      : `given for them by the station ${hms(sel.consent.at_ms)}`}</span
                  >
                {:else}
                  <span class="chk-box" data-consent="missing"><i class="missing"></i>No consent to record on file</span>
                {/if}
              </div>
              <div class="field">
                <label for="f-note">Note {presenter} sees, listeners do not</label>
                <textarea class="in ta" id="f-note" rows="2" bind:value={note} oninput={typed} onfocus={() => (editing = true)} onblur={() => { editing = false; save(); }}></textarea>
              </div>
              <div class="two-up">
                <div class="field">
                  <label for="f-name">Name on air <kbd>N</kbd></label>
                  <input class="in" id="f-name" bind:value={nameOnAir} oninput={typed} onfocus={() => (editing = true)} onblur={() => { editing = false; save(); }} placeholder={firstName(sel.name, sel.identity)} />
                </div>
                <div class="field">
                  <span class="lbl">Signed in as</span>
                  <div class="in ro">{sel.identity}</div>
                </div>
              </div>
            </div>
            {#if sel.topic}
              <div class="callin">
                <span class="caps">{who(sel)} typed on the player page{sel.consent ? ` · ${hms(sel.consent.at_ms)}` : ''}</span>
                <span>“{sel.topic}”</span>
              </div>
            {/if}
            {#if !sel.consent}
              <div class="notice" role="note">
                <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
                <span><b>No consent to record</b>{who(sel)} did not agree to be recorded, so they cannot go to the desk. Screen them if you like, then drop them; they can call again from the player page.</span>
              </div>
            {/if}
            {#if sel.banned_before}
              <div class="notice" role="note">
                <svg class="g-serious" aria-hidden="true"><use href="#g-fault" /></svg>
                <span><b>Banned before, {sel.banned_before.date}</b>{sel.banned_before.reason}. Nothing is on air yet: screen them or drop them.</span>
              </div>
            {/if}
            <div class="hear">
              <span
                >{who(sel)} hears <b>{talking ? 'you and the programme' : 'the programme'}</b>{talking ? '' : '; hold T to talk to them'}. Listeners hear <b>none of this</b>.</span
              >
              <span class="lvl"
                >{who(sel)} <span class="bar" role="img" aria-label="{who(sel)}'s level"><i style:width="{meterPct(sel.lufs)}%"></i></span></span
              >
            </div>
          {:else}
            <p class="empty">Select a caller: <kbd>↑</kbd> <kbd>↓</kbd> or <kbd>1</kbd>–<kbd>9</kbd>.</p>
          {/if}
        </div>
        <div class="foot">
          <div class="actions">
            <button
              type="button"
              class="btn primary"
              disabled={!sel || sel.dropped_ms !== null || !sel.consent}
              title={sel && !sel.consent ? 'No consent to record on file' : undefined}
              onclick={() => run({ type: 'send' })}
              >{sel?.declined ? 'Send again' : 'Send to desk'} <kbd>P</kbd></button
            >
            <button type="button" class="btn" disabled={!sel || sel.on_air} onclick={() => run({ type: 'back' })}>To queue <kbd>W</kbd></button>
            <span class="grow"></span>
            <button
              type="button"
              class="btn danger-hollow"
              class:armed={dropArmed}
              disabled={!sel || sel.dropped_ms !== null}
              onclick={() => run(dropArmed ? { type: 'drop' } : { type: 'arm', what: 'drop' })}
              >{dropArmed ? 'Press again' : 'Drop'} <kbd>⌃D</kbd><kbd>⌃D</kbd></button
            >
          </div>
          {#if sel?.declined}
            <div class="notice" role="status">
              <svg class="g-warning" aria-hidden="true"><use href="#g-desk-busy" /></svg>
              <span
                ><b>Desk busy: {presenter} is not taking callers</b>Declined {who(sel)} at {hms(sel.declined.at_ms)}{sel.declined.reason
                  ? `: “${sel.declined.reason}”`
                  : ''}. {who(sel)} stays ready and hears the programme.</span
              >
            </div>
          {/if}
        </div>
      </div>
    </section>

    <section class="col c2" aria-label="Chat moderation">
      <Needs />
      <StudioLane />
      <Statement />
    </section>

    <Room>
      {#if toast}
        <div class="toast {toast.tone}" data-panel="screener.toast" role={toast.tone === 'good' ? 'status' : 'alert'}>
          <div class="stripe"></div>
          <div class="tb">
            <svg aria-hidden="true"><use href="#{toast.glyph}" /></svg><span class="title" data-panel-title>{toast.title}</span><span class="m">{toast.msg}</span>
            {#if toast.acts.length}
              <span class="acts">
                {#each toast.acts as a (a.label)}
                  {#if a.talk}
                    <button
                      type="button"
                      class="btn sm quiet"
                      class:lit-talk={talkingToPresenter}
                      onpointerdown={() => run({ type: 'talk-seat', held: true })}
                      onpointerup={() => run({ type: 'talk-seat', held: false })}
                      ><svg class="g" aria-hidden="true"><use href="#g-talk" /></svg>{a.label} {#each a.keys as k, i (i)}<kbd>{k}</kbd>{/each}</button
                    >
                  {:else}
                    <button type="button" class="btn sm" onclick={a.go}>{a.label} {#each a.keys as k, i (i)}<kbd>{k}</kbd>{/each}</button>
                  {/if}
                {/each}
              </span>
            {/if}
          </div>
        </div>
      {/if}
      {#if chatToast}
        <div class="toast {chatToast.tone}" data-panel="screener.toast" data-chat="toast" role={chatToast.tone === 'good' ? 'status' : 'alert'}>
          <div class="stripe"></div>
          <div class="tb">
            <svg aria-hidden="true"><use href={chatToast.tone === 'good' ? '#g-good' : '#g-degraded'} /></svg><span class="title" data-panel-title
              >{chatToast.title}</span
            ><span class="m">{chatToast.msg}</span>
          </div>
        </div>
      {/if}
    </Room>
  </main>
</div>

<style>
  /* Built states the design page did not draw: empty panels, the read-only field, and the
     selected moderation item (an item holds buttons, so it is marked current, not selected). */
  :global(.screener .item[aria-current='true']) {
    background: var(--surface-2);
    border-color: var(--axis);
    box-shadow: inset 4px 0 0 var(--accent);
  }
  :global(.screener .bulk .grow) {
    flex: 1;
  }
  :global(.screener .item .countdown) {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
  }
  :global(.screener .empty) {
    margin: 0;
    padding: var(--s-3) var(--s-4);
    color: var(--ink-2);
    font-size: var(--fs-13);
    line-height: var(--lh-label);
  }
  :global(.screener .field .lbl) {
    font-size: var(--fs-12);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  :global(.screener .field input.in, .screener .field textarea.in) {
    font: inherit;
    font-size: var(--fs-14);
    width: 100%;
    resize: vertical;
  }
  :global(.screener .field .in.ro) {
    color: var(--ink-2);
    font-family: var(--font-mono);
    font-size: var(--fs-13);
  }
  :global(.screener .mode.armed-mode) {
    background: var(--status-critical);
    color: var(--status-ink-on-critical);
    border-color: transparent;
  }
  :global(.screener .qrow) {
    text-align: left;
  }
  /* A caller with no consent to record (M3): a hollow box ringed in the serious colour. */
  :global(.screener .chk-box i.missing) {
    border-color: var(--status-serious);
  }
  :global(.screener .tag.no-consent) {
    border-color: var(--status-serious);
    color: var(--ink);
  }
</style>
