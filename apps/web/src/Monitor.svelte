<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The monitor link (ADR-0280; hosts-in-their-own-studios.md section 4.5): the page a host
  // opens in a browser beside their own mixer. Its link carries a secret after the `#`, which
  // the browser never puts in an address it sends. The page says what state the host is in at
  // the size of the desk's lamp, what happens next and when, on the station's clock; plays
  // their own return; and offers the few things they may press.
  //
  // One rule above the rest: the page shows a state only while the station is telling it. When
  // views stop arriving it says so, and shows no lamp, no chip and no control until they return.
  import { onDestroy, onMount } from 'svelte';
  import Glyphs from '../../studio/src/lib/Glyphs.svelte';
  import BrandMark from './lib/BrandMark.svelte';
  import * as api from './lib/api.ts';
  import * as streams from './lib/streams.ts';
  import { deviceZone, hms, zoneLine } from './lib/monitor/clock.ts';
  import Controls from './lib/monitor/Controls.svelte';
  import { MonitorLink } from './lib/monitor/link.svelte.ts';
  import type { Listen } from './lib/monitor/listen.svelte.ts';
  import ListenPanel from './lib/monitor/Listen.svelte';
  import { look, setNight } from './lib/monitor/night.svelte.ts';
  import { barPct, counting, hurry, insertWords, lampOf, lead, nextRows, sourceWords, thenWords, titleOf, wayWord } from './lib/monitor/words.ts';

  const secret = (globalThis.location?.hash ?? '').replace(/^#/, '').trim();
  let listen = $state<Listen | null>(null);
  const link = new MonitorLink(secret, () => void listen?.stop());

  // The station's name and its relay's port are not in the host's view; the player's public
  // facts have both. Without them the header says WeaveSignal and the relay keeps LiveKit's port.
  let station = $state('WeaveSignal');
  let turnPort = 0;
  // The listeners' stream, for the while the host stands by and the desk's mix is silent.
  let stream = $state('');
  // The room's client is a large part of the page: fetched once the page is up, so it is there
  // by the time Start listening is pressed.
  let listening: Promise<Listen> | null = null;
  const own = deviceZone();
  const opusOk = typeof document === 'undefined' || document.createElement('audio').canPlayType('audio/ogg; codecs=opus') !== '';

  onMount(() => {
    link.start();
    if (!secret) return;
    void api
      .player()
      .then((p) => {
        station = p.station || station;
        turnPort = p.relay?.turn_tls_port ?? 0;
        stream = streams.pick(p.streams ?? [], streams.remembered(), p.stream_url ?? '', opusOk);
      })
      .catch(() => {});
    listening = import('./lib/monitor/listen.svelte.ts').then((m) => (listen = new m.Listen(secret, () => turnPort)));
    listening.catch(() => {});
  });
  onDestroy(() => {
    link.stop();
    void listen?.stop();
  });

  /** Start listening: the audio graph is made here, inside the press, so the browser lets it play. */
  function startListening() {
    let ctx: AudioContext | null = null;
    try {
      ctx = new AudioContext();
      void ctx.resume().catch(() => {});
    } catch {
      ctx = null;
    }
    void listening?.then((l) => l.start(ctx));
  }

  const view = $derived(link.view);
  /** The station is telling the page the host's state now. */
  const known = $derived(link.standing === 'live');
  const zone = $derived(view?.station.zone ?? 'UTC');
  const now = $derived(link.now ?? 0);
  const lamp = $derived(view && known ? lampOf(view.state) : null);
  const says = $derived(view ? lead(view.says) : { lead: '', rest: '' });
  const then = $derived(view ? thenWords(view) : '');
  const c = $derived(view && known ? counting(view, now, zone) : null);
  const urgent = $derived(c ? hurry(c, now) : null);
  const source = $derived(view ? sourceWords(view, now, zone) : null);
  const rows = $derived(view ? nextRows(view, now, zone, known) : []);
  const insert = $derived(view?.insert ? insertWords(view.insert, view, now, zone) : null);
  const gone = $derived(lead(link.words));

  // The count's bar runs from when this page first saw the count; a reload keeps that.
  const BAR_KEPT = 'weavesignal.monitor.count';
  let barKey = '';
  let barSince = 0;
  const bar = $derived.by(() => {
    if (!c) return 0;
    const key = `${c.kind}:${c.at_ms}`;
    if (key !== barKey) {
      barKey = key;
      barSince = now;
      try {
        const kept = JSON.parse(sessionStorage.getItem(BAR_KEPT) ?? 'null');
        if (kept?.key === key && Number.isFinite(kept.since) && kept.since <= now) barSince = kept.since;
        else sessionStorage.setItem(BAR_KEPT, JSON.stringify({ key, since: now }));
      } catch {
        // no storage: the bar runs from now
      }
    }
    return barPct(c, now, barSince);
  });

  $effect(() => {
    if (view) document.title = titleOf(view);
  });
</script>

<Glyphs />
<div class="page mon" data-standing={link.standing}>
  <header class="top">
    <BrandMark />
    <span class="station">{station}</span>
    <span class="where">Monitor</span>
    <span class="spacer"></span>
    <span class="seg" role="group" aria-label="Theme">
      <button type="button" aria-pressed={!look.night} onclick={() => setNight(false)}>Device</button>
      <button type="button" aria-pressed={look.night} onclick={() => setNight(true)}>Night</button>
    </span>
  </header>

  <main class="main" aria-label="Your monitor">
    {#if link.standing === 'gone'}
      <!-- A link that is not good shows its one sentence and nothing else. -->
      <section class="card alone" role="alert" data-says="gone">
        <h1>{gone.lead}</h1>
        {#if gone.rest}<p>{gone.rest}</p>{/if}
      </section>
    {:else if link.standing === 'refused'}
      <section class="card alone" role="alert" data-says="refused">
        <h1>{gone.lead}</h1>
        {#if gone.rest}<p>{gone.rest}</p>{/if}
        <p class="small">This page tries again by itself.</p>
      </section>
    {:else if !view}
      <section class="card alone" role="status" data-says="opening">
        <h1>{link.standing === 'lost' ? 'This page cannot reach the station.' : 'Opening your monitor…'}</h1>
        {#if link.standing === 'lost'}<p>Trying again…</p>{/if}
      </section>
    {:else}
      <h1 class="sr-only">Your monitor for {view.show_name}</h1>
      <section class="card lampcard is-{lamp?.kind ?? 'away'}" data-panel="monitor.lamp" data-state={known ? view.state : 'away'}>
        <div class="status" role="status" aria-live="polite" aria-atomic="true">
          {#if known}
            {#if lamp}
              <span class="biglamp {lamp.kind}"><svg aria-hidden="true"><use href="#{lamp.chip.glyph}" /></svg>{lamp.chip.word}</span>
            {/if}
            <div class="say">
              <span class="caps">{titleOf(view)}</span>
              <p><b>{says.lead}</b> {says.rest}</p>
              {#if then}<p class="then">{then}</p>{/if}
            </div>
          {:else}
            <div class="say">
              <span class="caps">{titleOf(view)}</span>
              <p><b>This page has lost the station.</b> Trying again…</p>
              <p class="then">
                It cannot say what state you are in until it is back. Your software sends on a connection of its own, which this page does not touch.{#if link.heardAt !== null}
                  {' '}Last heard at {hms(link.heardAt, zone)}.{/if}
              </p>
            </div>
          {/if}
        </div>
        {#if c}
          <div class="cnt" role="timer" data-hurry={urgent?.level} aria-label="{c.label}{c.figure ? `: ${c.figure}` : ''}{urgent ? `. ${urgent.word}` : ''}">
            <span class="caps">{c.label}</span>
            {#if c.figure}<b class="num">{c.figure}</b>{/if}
            {#if urgent}
              <!-- The colour is never the only sign: the word says it too. No glyph: the clock's is DELAY's, and no state is new. -->
              <span class="hurry">{urgent.word}</span>
            {/if}
            {#if c.small}<span class="small">{c.small}</span>{/if}
            {#if c.figure}<div class="bar" aria-hidden="true"><i style:width="{bar}%"></i></div>{/if}
          </div>
        {/if}
      </section>

      <div class="mgrid">
        <div class="mcol">
          <section class="card clockcard" data-panel="monitor.clock" aria-label="Station time">
            <span class="caps">Station time</span>
            <b class="time num">{hms(now, zone)}</b>
            <span class="small">{zoneLine(now, zone, own)}</span>
          </section>
          <Controls {link} {view} {known} {now} {zone} />
        </div>

        <div class="mcol">
          <ListenPanel {view} {known} {listen} {stream} onstart={startListening} />
          {#if insert}
            <section class="card insert" data-panel="monitor.insert" aria-label="The station's next insert">
              <svg aria-hidden="true"><use href="#g-once" /></svg>
              <div><b>{insert.title}</b><span class="small">{insert.detail}</span></div>
            </section>
          {/if}
        </div>

        <div class="mcol">
          {#if rows.length}
            <section class="card" data-panel="monitor.next" aria-labelledby="next-h">
              <h2 id="next-h" class="sr-only">Before and after you</h2>
              <ol class="nn">
                {#each rows as r (r.where)}
                  <li>
                    <span class="caps">{r.where}</span>
                    <span class="what">
                      <span><b>{r.title}</b><span class="small">{r.detail}</span></span>
                      {#if r.chip}
                        <span class="state sm {r.chip.cls}"><svg class={r.chip.glyphCls} aria-hidden="true"><use href="#{r.chip.glyph}" /></svg>{r.chip.word}</span>
                      {/if}
                    </span>
                  </li>
                {/each}
              </ol>
            </section>
          {/if}
          <section class="card" data-panel="monitor.source" data-connected={known && source ? source.connected : 'unknown'} aria-labelledby="source-h">
            <h2 id="source-h">The station hears you</h2>
            {#if !known || !source}
              <p class="small">This page cannot say what the station hears until it reaches the station again.</p>
            {:else}
              {#if source.problem}<p class="problem" role="alert" data-says="source-problem">{source.problem}</p>{/if}
              {#if source.checks.length}
                <ul class="checks">
                  {#each source.checks as k (k.title)}
                    <li data-mark={k.mark}>
                      {#if k.mark === 'read'}
                        <!-- A reading, with no verdict: no state's shape stands beside it. -->
                        <span class="dot" aria-hidden="true"></span>
                      {:else}
                        <svg class={k.mark === 'ok' ? 'g-good' : 'g-accent'} aria-hidden="true"><use href={k.mark === 'ok' ? '#g-good' : '#g-delay'} /></svg>
                      {/if}
                      <span><b>{k.title}<span class="sr-only">{k.mark === 'ok' ? ': yes' : k.mark === 'wait' ? ': not yet' : ''}</span></b><span class="small">{k.detail}</span></span>
                    </li>
                  {/each}
                </ul>
              {/if}
              {#if source.rows.length}
                <dl class="facts">
                  {#each source.rows as [k, v] (k)}<dt>{k}</dt><dd>{v}</dd>{/each}
                </dl>
              {/if}
              {#if source.meter !== null}
                <span class="level" role="img" aria-label="Your level at the station"><i style:width="{source.meter}%"></i></span>
              {/if}
              {#if source.icecast}
                <p class="small">Icecast: your sound reaches the station about 2 s after you play it. Right for a show on your own; for callers, use SRT.</p>
              {/if}
            {/if}
          </section>
        </div>
      </div>

      <details class="trouble card" data-panel="monitor.details">
        <summary>Your connection details, and help</summary>
        <div>
          <dl class="facts">
            <dt>Your details</dt>
            <dd>The server, the port and your show key are in the message the station sent you. This page does not hold them.</dd>
            <dt>Your key</dt>
            <dd>Not kept where it can be read. Ask the station for new details if you have lost it.</dd>
            {#if known && view.way}
              <dt>Way in now</dt>
              <dd>{[wayWord(view.way), view.format ?? ''].filter(Boolean).join(' · ')}</dd>
            {/if}
          </dl>
          <h3>Having trouble?</h3>
          <ul>
            <li><b>Your software says it failed.</b> <i>The station hears you</i> gives the station's reason, in words.</li>
            <li><b>No sound from your monitor.</b> Raise the volume here and on this device. Nothing plays before you are standing by.</li>
            <li>
              <b>Your monitor will not connect.</b> Some networks block it.
              {#if listen?.relay}It is using the station's relay.{:else}The station's relay gets through them.{/if}
              {#if listen && !listen.relay && listen.hearing !== 'idle'}
                <button type="button" class="btn wide" data-act="relay" onclick={() => listen?.useRelay()}>Reconnect through the relay</button>
              {/if}
            </li>
            <li><b>Two browsers</b> can listen on this link at once. A third is refused.</li>
            <li><b>This link is yours alone.</b> Whoever has it can end your show: do not pass it on.</li>
          </ul>
        </div>
      </details>
    {/if}
  </main>
</div>
