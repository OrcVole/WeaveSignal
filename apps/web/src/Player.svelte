<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The listener player page (ADR-0006, ADR-0058): the show, the stream, and "call the studio".
  // The lobby opens in place, so the stream keeps playing while the listener types; the call
  // replaces the stream (the studio's sound arrives through the call), and "back to the show"
  // brings it back. Consent to be broadcast and recorded is required before a call is placed.
  import { onDestroy, onMount, tick } from 'svelte';
  import type { Msg } from '../../../packages/ui/chat.ts';
  import { CHIP } from '../../../packages/ui/states.ts';
  import Glyphs from '../../studio/src/lib/Glyphs.svelte';
  import BrandMark from './lib/BrandMark.svelte';
  import { brand } from './lib/brand.svelte.ts';
  import * as api from './lib/api.ts';
  import type { Call } from './lib/call.svelte.ts';
  import CallPanel from './lib/CallPanel.svelte';
  import * as challenge from './lib/challenge.ts';
  import { useTurnPort } from './lib/turn.ts';
  import ChatPanel from './lib/chat/ChatPanel.svelte';
  import OnAirLine from './lib/chat/OnAirLine.svelte';
  import { check, firstError, request, type LobbyErrors } from './lib/lobby.ts';
  import MicCheck from './lib/MicCheck.svelte';
  import * as store from './lib/store.ts';
  import * as streams from './lib/streams.ts';
  import { choose, detect, positionAt } from './lib/watch.ts';
  import type HlsType from 'hls.js';

  let info = $state<api.PlayerInfo | null>(null);
  let loadProblem = $state<string | null>(null);
  let view = $state<'listen' | 'lobby' | 'call'>('listen');
  let call = $state<Call | null>(null);
  let audio = $state<HTMLAudioElement | undefined>();
  /** The desk's HLS, picture or sound alone (M5); the Icecast stream stays on `audio`. */
  let screen = $state<HTMLVideoElement | undefined>();
  let playing = $state(false);
  let audioOnly = $state(false);
  let hls: HlsType | null = null;
  const caps = detect();
  /** The stream the listener chose (ADR-0232), kept on this device; Opus until they choose. */
  let chosenStream = $state(streams.remembered());
  const opusOk = typeof document === 'undefined' || document.createElement('audio').canPlayType('audio/ogg; codecs=opus') !== '';
  const offered = $derived(info?.streams ?? []);
  const streamUrl = $derived(info ? streams.pick(offered, chosenStream, info.stream_url, opusOk) : '');
  const choice = $derived(info ? choose(info.video, streamUrl, caps, audioOnly) : null);
  let streamProblem = $state<string | null>(null);
  let kept = $state(store.kept(Date.now()));
  /** A chat message the studio put on air, shown as text beside the stream (D32). */
  let onAir = $state<Msg | null>(null);

  let name = $state('');
  let topic = $state('');
  let consent = $state(false);
  let errors = $state<LobbyErrors>({});
  let sending = $state(false);
  let sendProblem = $state<string | null>(null);
  let lobbyHeading = $state<HTMLElement | undefined>();
  // Call-in's challenge (ADR-0006): fetched and answered in a worker as the lobby opens, so by
  // the time the caller has typed their name it is long done. Nothing to see or solve.
  let check$: Promise<challenge.Solved | null> | null = null;
  let checking = $state(false);
  function startCheck() {
    check$ = challenge.prepare();
    check$.catch(() => {});
  }
  async function answered(): Promise<challenge.Solved | null> {
    let s = await (check$ ?? Promise.reject(new Error('no check'))).catch(() => null);
    if (!challenge.fresh(s)) {
      startCheck();
      s = await check$!;
    }
    return s;
  }

  async function load() {
    try {
      info = await api.player();
      loadProblem = null;
      document.title = `${info.station}${info.show ? ` · ${info.show}` : ''}`;
    } catch {
      loadProblem = "The station's page could not load. Check your connection and reload.";
    }
  }
  let poll: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    void load();
    poll = setInterval(() => view !== 'call' && load(), 15_000);
  });
  onDestroy(() => clearInterval(poll));

  function stopHls() {
    hls?.destroy();
    hls = null;
  }

  /** Play an HLS playlist on the screen element; `at` (ms since the epoch) resumes the same
   * instant after the audio-only toggle, by the segments' program date-time. hls.js loads only
   * here, so a listener who never presses play never fetches it. No worker: the pages' CSP
   * allows workers from the page's origin only, and fMP4 needs no transmuxing. */
  async function startHls(url: string, at: number | null) {
    const { default: Hls } = await import('hls.js');
    stopHls();
    if (!screen) return;
    const h = new Hls({ preferManagedMediaSource: true, liveSyncDurationCount: 3, enableWorker: false });
    hls = h;
    if (at !== null) {
      h.once(Hls.Events.LEVEL_LOADED, (_e, d) => {
        const p = positionAt(
          d.details.fragments.map((f) => ({ start: f.start, duration: f.duration, programDateTime: f.programDateTime })),
          at,
        );
        if (p !== null && screen) screen.currentTime = p;
      });
    }
    h.on(Hls.Events.ERROR, (_e, d) => {
      if (d.fatal) {
        streamProblem = 'The stream stopped. Press play to try again.';
        stopHls();
      }
    });
    h.loadSource(url);
    h.attachMedia(screen);
    await screen.play();
  }

  function pauseAll() {
    audio?.pause();
    screen?.pause();
    stopHls();
  }

  async function toggle() {
    if (!choice || !choice.url) return;
    streamProblem = null;
    if (playing) {
      pauseAll();
      return;
    }
    try {
      if (choice.mode === 'icecast') {
        if (!audio) return;
        audio.src = choice.url;
        await audio.play();
      } else {
        await startHls(choice.url, null);
      }
    } catch {
      streamProblem = "The stream didn't start. Try again in a moment.";
    }
  }

  /** Another stream (ADR-0232): kept for next time, and played at once if the stream plays. */
  async function chooseStream(url: string) {
    chosenStream = url;
    streams.remember(url);
    if (playing && audio && choice?.mode === 'icecast') {
      streamProblem = null;
      try {
        audio.src = url;
        await audio.play();
      } catch {
        streamProblem = "The stream didn't start. Try again in a moment.";
      }
    }
  }

  /** Audio only, and back: the same segments, from the same instant (ADR-0026). */
  async function setAudioOnly(on: boolean) {
    const at = hls?.playingDate?.getTime() ?? null;
    audioOnly = on;
    if (playing && choice && choice.mode !== 'icecast') {
      try {
        await startHls(choice.url, at);
      } catch {
        streamProblem = "The stream didn't start. Try again in a moment.";
      }
    }
  }

  async function openLobby() {
    view = 'lobby';
    sendProblem = null;
    startCheck();
    await tick();
    lobbyHeading?.focus();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!info) return;
    sendProblem = null;
    const lim = { name_max: info.call_in.name_max, topic_max: info.call_in.topic_max };
    errors = check({ name, topic, consent }, lim);
    const req = request({ name, topic, consent }, lim, info.call_in.consent_version);
    if (!req) {
      const f = firstError(errors);
      if (f) document.getElementById(`f-${f}`)?.focus();
      return;
    }
    sending = true;
    try {
      let joined: api.Joined | null = null;
      // One retry with a fresh challenge if the station refused the first (expired, restarted).
      for (let attempt = 0; !joined; attempt++) {
        checking = true;
        const solved = await answered();
        checking = false;
        try {
          joined = await api.callIn(solved ? { ...req, challenge: solved.answer } : req);
        } catch (err) {
          check$ = null;
          if (attempt === 0 && err instanceof api.ApiError && err.field === 'challenge') continue;
          throw err;
        }
      }
      await startCall(joined);
    } catch (err) {
      if (err instanceof api.ApiError && err.field) {
        errors = { [err.field]: err.message };
        if (err.field === 'challenge') {
          sendProblem = err.message;
          errors = {};
        } else if (err.field === 'consent' && err.status === 409) {
          consent = false;
          await load();
        }
        document.getElementById(`f-${err.field}`)?.focus();
      } else {
        sendProblem = err instanceof Error ? err.message : String(err);
      }
    } finally {
      sending = false;
      checking = false;
    }
  }

  /** The room's client loads only when a call starts, so the player itself stays light on a phone. */
  async function startCall(joined: api.Joined) {
    pauseAll();
    const { Call } = await import('./lib/call.svelte.ts');
    if (info) useTurnPort(info.relay.turn_tls_port);
    call = new Call(joined);
    view = 'call';
    kept = null;
    void call.start();
  }

  async function rejoin() {
    if (!kept) return;
    try {
      await startCall(await api.passToken(kept.pass));
    } catch (err) {
      store.forget();
      kept = null;
      sendProblem = err instanceof Error ? err.message : String(err);
    }
  }

  function backToShow() {
    call = null;
    view = 'listen';
    name = topic = '';
    consent = false;
    errors = {};
    void load();
    void toggle();
  }

  const live = $derived(info?.show_on_air ?? false);
  const chip = $derived(live ? CHIP.onAir : { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Off air' });
</script>

<Glyphs />
<div class="page">
  <!-- The station's banner (ADR-0237), when it has set one. Decorative: the name is in the header. -->
  {#if brand.banner}<img class="banner" src={brand.banner.url} width={brand.banner.width} height={brand.banner.height} alt="" decoding="async" />{/if}
  <header class="top">
    <BrandMark />
    <span class="station">{info?.station ?? ' '}</span>
    <span class="spacer"></span>
    <!-- During a call the banner says where the caller stands; a red ON AIR up here would read as theirs. -->
    {#if info && view !== 'call'}<span class="state {chip.cls}" data-show-on-air={live}><svg class={chip.glyphCls} aria-hidden="true"><use href="#{chip.glyph}" /></svg>{chip.word}</span>{/if}
  </header>

  <main class="main" aria-label="Player">
    {#if loadProblem}
      <p class="problem" role="alert">{loadProblem}</p>
    {/if}

    <section class="now" aria-labelledby="show-title">
      <span class="caps">Now</span>
      <h1 id="show-title">{info?.show || info?.station || 'Loading…'}</h1>
      {#if info?.presenter}
        <p class="muted">with {info.presenter}{info.presenter_live ? ', live now' : ''}</p>
      {/if}
      {#if view !== 'call'}
        <button
          type="button"
          class="btn primary big wide"
          aria-pressed={playing}
          disabled={!choice?.url}
          onclick={toggle}
          ><svg aria-hidden="true" viewBox="0 0 18 18"
            >{#if playing}<path d="M5 3h3v12H5zM10 3h3v12h-3z" fill="currentColor" />{:else}<path d="M5 2.5v13l11-6.5z" fill="currentColor" />{/if}</svg
          >{playing ? 'Pause' : choice?.mode === 'video' ? 'Watch live' : 'Listen live'}</button
        >
        {#if choice?.canToggle}
          <button type="button" class="btn quiet wide" aria-pressed={audioOnly} onclick={() => setAudioOnly(!audioOnly)}
            >{audioOnly ? 'Show the picture' : 'Audio only'}</button
          >
        {/if}
        {#if choice?.mode === 'icecast' && offered.length > 1}
          <fieldset class="group streams" data-player="streams">
            <legend class="caps">Stream</legend>
            {#each offered as s (s.url)}
              {@const why = streams.hint(s, offered)}
              <label class="check"
                ><input type="radio" name="stream" value={s.url} checked={streamUrl === s.url} onchange={() => chooseStream(s.url)} data-codec={s.codec} data-kbps={s.kbps} /><span
                  >{streams.label(s)}{#if why}<span class="small hint"> · {why}</span>{/if}</span
                ></label
              >
            {/each}
          </fieldset>
        {/if}
        {#if choice?.note}<p class="small" role="status">{choice.note}</p>{/if}
        {#if info && !choice?.url}<p class="small">The stream is not set up at this station yet.</p>{/if}
        {#if streamProblem}<p class="problem" role="alert">{streamProblem}</p>{/if}
      {/if}
      <OnAirLine m={onAir} />
      {#if choice && choice.mode !== 'icecast'}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={screen}
          class="screen"
          class:hidden={choice.mode === 'audio' || !playing}
          playsinline
          preload="none"
          aria-label="The studio's picture"
          onplay={() => (playing = true)}
          onpause={() => (playing = false)}
        ></video>
      {/if}
      <audio bind:this={audio} preload="none" onplay={() => (playing = true)} onpause={() => (playing = false)} onerror={() => playing && (streamProblem = 'The stream stopped. Press Listen live to try again.')}></audio>
    </section>

    {#if view === 'call' && call}
      <CallPanel {call} relayPort={info?.relay.turn_tls_port ?? 5349} onclose={backToShow} />
    {:else if view === 'lobby' && info}
      <form class="card lobby" novalidate onsubmit={submit} aria-labelledby="lobby-h">
        <h2 id="lobby-h" tabindex="-1" bind:this={lobbyHeading}>Call the studio</h2>
        <p class="small">The producer answers first. You're not on air until the presenter brings you in, and this page tells you when you are.</p>

        <div class="field">
          <label for="f-name">Your name</label>
          <span class="hint" id="name-hint">Said on air if you go on. A first name is fine.</span>
          <input
            id="f-name"
            type="text"
            autocomplete="given-name"
            maxlength={info.call_in.name_max}
            bind:value={name}
            aria-invalid={!!errors.name}
            aria-describedby="name-hint{errors.name ? ' name-err' : ''}"
          />
          {#if errors.name}<span class="err" id="name-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{errors.name}</span>{/if}
        </div>

        <div class="field">
          <label for="f-topic">What would you like to talk about?</label>
          <span class="hint" id="topic-hint">One line for the producer.</span>
          <input
            id="f-topic"
            type="text"
            autocomplete="off"
            maxlength={info.call_in.topic_max}
            bind:value={topic}
            aria-invalid={!!errors.topic}
            aria-describedby="topic-hint{errors.topic ? ' topic-err' : ''}"
          />
          <span class="count num" aria-hidden="true">{[...topic].length}/{info.call_in.topic_max}</span>
          {#if errors.topic}<span class="err" id="topic-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{errors.topic}</span>{/if}
        </div>

        <fieldset class="consent" class:invalid={!!errors.consent}>
          <legend>Recording</legend>
          <p id="consent-words" data-version={info.call_in.consent_version}>{info.call_in.consent_text}</p>
          <label class="check">
            <input
              id="f-consent"
              type="checkbox"
              bind:checked={consent}
              required
              aria-invalid={!!errors.consent}
              aria-describedby="consent-words{errors.consent ? ' consent-err' : ''}"
            />
            <span>I agree to my call being broadcast and recorded</span>
          </label>
          {#if errors.consent}<span class="err" id="consent-err"><svg aria-hidden="true"><use href="#g-fault" /></svg>{errors.consent}</span>{/if}
        </fieldset>

        <MicCheck />
        <p class="note"><svg aria-hidden="true"><use href="#g-mic" /></svg><span>Headphones help: they stop the show echoing back through your microphone.</span></p>

        {#if sendProblem}<p class="problem" role="alert">{sendProblem}</p>{/if}
        <p class="small" role="status" data-checking={checking}>{checking ? 'Getting your call ready: your browser is doing a quick check. This takes a few seconds on some phones.' : ''}</p>
        <button type="submit" class="btn primary big wide" disabled={sending} aria-busy={sending}>{checking ? 'Getting ready…' : sending ? 'Calling…' : 'Call the studio'}</button>
        <button type="button" class="btn quiet wide" onclick={() => (view = 'listen')}>Not now</button>
      </form>
    {:else if info}
      <section class="card" aria-labelledby="callin-h">
        <h2 id="callin-h">Talk to the studio</h2>
        {#if kept}
          <p>You were on a call as <b>{kept.name}</b>. Your place in the queue is held for a few minutes.</p>
          <button type="button" class="btn primary big wide" onclick={rejoin}>Rejoin your call</button>
          <button type="button" class="btn quiet wide" onclick={() => { store.forget(); kept = null; }}>Forget it</button>
        {:else if info.call_in.open}
          <p class="small">Call from this page with your phone or computer's microphone. No phone number needed.</p>
          <button type="button" class="btn primary big wide" onclick={openLobby}
            ><svg aria-hidden="true"><use href="#g-mic" /></svg>Call the studio</button
          >
        {:else}
          <p class="small" role="status">{info.call_in.why_not}</p>
          <button type="button" class="btn big wide" disabled>Call the studio</button>
        {/if}
        {#if sendProblem && view === 'listen'}<p class="problem" role="alert">{sendProblem}</p>{/if}
      </section>
    {/if}

    <!-- The chat stays mounted through the lobby and a call, so it keeps its place and session. -->
    <ChatPanel {playing} inCall={view === 'call'} oninvite={() => view !== 'call' && openLobby()} onfeatured={(m) => (onAir = m)} />
  </main>
</div>

<style>
  .main {
    display: grid;
    gap: var(--s-4);
  }
  .now {
    display: grid;
    gap: var(--s-2);
  }
  .now h1 {
    font-size: var(--fs-32);
    letter-spacing: -0.01em;
  }
  .screen {
    width: 100%;
    aspect-ratio: 16 / 9;
    background: var(--bg-sunk, #000);
    border-radius: var(--r-2, 6px);
  }
  .screen.hidden {
    display: none;
  }
  .streams {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
  }
  .streams legend {
    padding: 0;
    margin-bottom: var(--s-1);
  }
  .lobby h2:focus {
    outline: none;
  }
  .lobby h2:focus-visible {
    outline: var(--focus-ring);
  }
</style>
