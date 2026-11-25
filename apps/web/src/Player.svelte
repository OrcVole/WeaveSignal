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
  import { useStationIce } from './lib/turn.ts';
  import ChatPanel from './lib/chat/ChatPanel.svelte';
  import OnAirLine from './lib/chat/OnAirLine.svelte';
  import { check, firstError, request, type LobbyErrors } from './lib/lobby.ts';
  import { CLOSED_WHILE_FILLING, NEEDS_HTTPS, closedLine, closedUnderForm, talkNeedsHttps } from './lib/lines.ts';
  import MicCheck from './lib/MicCheck.svelte';
  import * as store from './lib/store.ts';
  import * as streams from './lib/streams.ts';
  import { choose, detect, playLabel, positionAt } from './lib/watch.ts';
  import { Keyed } from './lib/keyed/keyed.svelte.ts';
  import KeyPanel from './lib/keyed/KeyPanel.svelte';
  import * as keyedRoutes from './lib/keyed/routes.ts';
  import { BADGE, keyedNow, opens, pressToHear } from './lib/keyed/words.ts';
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
  // A listener's key (keyed shows, 1.3.2): which shows need one, the key kept on this device and
  // what the station said of it. While the show that is on needs a key and this device holds one
  // that opens it, the player plays the keyed stream; everyone else hears what the station plays
  // for the public. Every keyed address is made in lib/keyed/routes.ts.
  const k = new Keyed();
  /** The keyed show on now: the player's facts name it by id while its window is open. */
  const keyedShow = $derived(keyedNow(k.shows, info, k.session?.show ?? null));
  /** This device holds a token for the keyed show that is on. */
  const holder = $derived(!!k.session && !!keyedShow && k.session.show.id === keyedShow.id && opens(k.session.key, keyedShow));
  /** The public HLS answered that a keyed show holds it back (the window and the lag after it):
   *  until then, the public sound is played in its place. */
  let hlsHeldUntil = $state(0);
  const streamUrl = $derived.by(() => {
    if (!info) return '';
    // The listener's own choice of stream is kept by its public address.
    const pub = streams.pick(info.streams ?? [], chosenStream, info.stream_url, opusOk);
    return holder && pub ? keyedRoutes.keyedStream(k.session!.urls, pub) : pub;
  });
  /** The picture, as this listener may have it: a key for the sound alone gets no picture, and
   *  while a keyed show holds the public picture back, someone without its key hears the public
   *  sound (the station's slate) instead. */
  const video = $derived.by(() => {
    const v = info?.video ?? null;
    if (!v) return v;
    if (holder) return k.session!.key.media === 'sound' ? null : keyedRoutes.keyedHls(k.session!.urls);
    if (info?.keyed || hlsHeldUntil > Date.now()) return null;
    return v;
  });
  const choice = $derived(info ? choose(video, streamUrl, caps, audioOnly) : null);
  /** The stream chosen, by its public address: what the Stream choices are told apart by. */
  const publicChoice = $derived(info ? streams.pick(info.streams ?? [], chosenStream, info.stream_url, opusOk) : '');
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
  /** The chat is open to write in (the chat panel says): the way to reach a show that takes no calls. */
  let chatOpen = $state(false);
  /** Plain HTTP on a home-network address: no microphone, so no call (1.3.1). */
  const insecure = talkNeedsHttps(typeof window === 'undefined' ? undefined : window.isSecureContext);
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
      const show = keyedNow(k.shows, info, k.session?.show ?? null)?.name || info.show;
      document.title = `${info.station}${show ? ` · ${show}` : ''}`;
    } catch {
      loadProblem = "The station's page could not load. Check your connection and reload.";
    }
  }
  let poll: ReturnType<typeof setInterval> | undefined;
  let polls = 0;
  /** This is `/listen/`, the page a key's link leads to: opened without its key, it offers the
   *  box to type or paste one (the operator, 2026-11-25), and a key accepted there plays. */
  const keyPage = typeof location !== 'undefined' && keyedRoutes.isKeyPage(location.pathname);
  /** The key this page came with (in its link, or kept from last time) has been asked about, or
   *  there was none: only then does `/listen/` offer its box. Someone who opened the link, all of
   *  it, is never asked for the link while the station is still answering. */
  let keyAsked = $state(false);
  /** A key was accepted in that box and the browser would not start the sound by itself. */
  let pressNote = $state(false);
  /** The stream is the keyed one: so a key that is accepted, or stops, is heard as it should be. */
  let playingKeyed = false;
  /** The keyed show the keyed stream that plays is for. */
  let playingShow: string | null = null;
  let keyCut = $state(false);
  onMount(() => {
    // A key's link carries the key after the `#`: it is taken from the address at once, so a
    // copied address does not carry it onward, and no server ever saw it.
    const linked = keyedRoutes.keyInAddress(location.hash);
    if (linked) history.replaceState(null, '', keyedRoutes.withoutKey(location.href));
    k.onCut = () => {
      // A key that stops mid-listen is never silence without words: the stream stops, and the
      // key's panel says why.
      if (playingKeyed) {
        pauseAll();
        keyCut = true;
      }
    };
    void Promise.all([load(), k.loadShows()])
      .then(() => {
        const show = keyedNow(k.shows, info)?.id ?? null;
        return linked ? k.enter(linked, show) : k.resume(show);
      })
      .catch(() => false)
      .finally(() => (keyAsked = true));
    poll = setInterval(() => {
      if (view === 'call') return;
      void load();
      // Which shows need a key changes with the schedule, not by the second.
      if (++polls % 4 === 0) void k.loadShows();
    }, 15_000);
  });
  onDestroy(() => {
    clearInterval(poll);
    k.stop();
  });
  // The show changed: which shows need a key is read again, and the key is asked for the new one.
  let lastShow: string | undefined;
  $effect(() => {
    const show = `${info?.show_id ?? ''}|${info?.keyed ?? false}`;
    if (show === lastShow) return;
    const first = lastShow === undefined;
    lastShow = show;
    if (!first) void k.loadShows();
  });
  $effect(() => {
    void k.follow(keyedShow?.id ?? null);
  });
  // A token for a show later on: the page looks again as its window opens, not at the next poll.
  $effect(() => {
    const at = k.session?.starts_ms ?? 0;
    const wait = at - Date.now();
    if (wait <= 0 || wait > 86_400_000) return;
    const t = setTimeout(() => void Promise.all([load(), k.loadShows()]), wait + 250);
    return () => clearTimeout(t);
  });
  // A key accepted while the public stream plays: the show takes its place (the effect on
  // `playKey` below), and words about a key that stopped are no longer true.
  $effect(() => {
    if (holder) keyCut = false;
  });
  // What there is to play changed under a stream that plays (a key accepted, a keyed show began
  // so the public picture is held back, another stream chosen): the stream starts again on what
  // there is now. A keyed stream that its show has left is not cut short: the station plays its
  // holders the show to its end and then ends it, and the page goes back to the public programme
  // then (audioStopped).
  const playKey = $derived(choice ? `${choice.mode}|${holder ? `k${keyedShow?.id}` : 'p'}|${choice.mode === 'icecast' ? publicChoice : ''}` : '');
  let playingKey = '';
  $effect(() => {
    if (!playing || view === 'call' || !playKey || playKey === playingKey) return;
    // A keyed stream is never cut short for another show (its own show's end, the next keyed
    // show): the station ends it when its show has been heard to the end.
    if (playingKeyed && (!holder || keyedShow?.id !== playingShow)) return;
    void restart();
  });

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
    const h = new Hls({ preferManagedMediaSource: true, liveSyncDurationCount: 3, enableWorker: false, ...(holder ? { xhrSetup: keyedRequest } : {}) });
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
      if (!d.fatal) return;
      stopHls();
      const refused = d.response?.code === 403;
      if (playingKeyed) return void keyedStopped();
      if (refused && !holder) {
        // A keyed show holds the public picture back: the public sound plays meanwhile.
        hlsHeldUntil = Date.now() + 40_000;
        setTimeout(() => (hlsHeldUntil = 0), 41_000);
        return void restart();
      }
      streamProblem = 'The stream stopped. Press play to try again.';
    });
    h.loadSource(url);
    h.attachMedia(screen);
    await screen.play();
  }

  /** hls.js asks for every playlist and segment under the address it was given; a keyed one
   *  carries the token of that moment in its path, so each request is sent with the token the
   *  page holds now. */
  function keyedRequest(xhr: XMLHttpRequest, url: string) {
    const t = k.session?.token;
    xhr.open('GET', t ? keyedRoutes.withToken(url, t) : url, true);
  }

  /** Start what `choice` says, from now. */
  /** The listener asked for the stream and has not paused it. */
  let listening = false;
  async function start() {
    if (!choice || !choice.url) return;
    listening = true;
    playingKeyed = holder;
    playingShow = holder ? (keyedShow?.id ?? null) : null;
    playingKey = playKey;
    if (choice.mode === 'icecast') {
      if (!audio) return;
      audio.src = choice.url;
      await audio.play();
    } else {
      await startHls(choice.url, null);
    }
  }

  /** The stream again, on its new address (a key accepted, or a keyed stream the edge ended
   *  when its token ran out: the page holds a fresh one by then). */
  let lastRestart = 0;
  async function restart() {
    lastRestart = Date.now();
    streamProblem = null;
    try {
      audio?.pause();
      stopHls();
      await start();
    } catch {
      streamProblem = "The stream didn't start. Try again in a moment.";
    }
  }

  /** The audio stream stopped by itself. (`listening`, not `playing`: a stream that ends is
   *  paused by the browser before it says it ended.) */
  function audioStopped() {
    if (!listening) return;
    if (playingKeyed) return void keyedStopped();
    playing = listening = false;
    streamProblem = `The stream stopped. Press ${playLabel(choice?.mode)} to try again.`;
  }

  /** A keyed stream stopped. The station ends one at once when its key is revoked or paused, and
   *  after its show has been played to its end: the key is asked about now, so a key that stopped
   *  working is said in words (the key's panel), and otherwise the stream is taken up again, on
   *  the keyed show while it is on and on the public programme after it. */
  let keyedRetries = 0;
  async function keyedStopped() {
    const ok = await k.check();
    if (keyCut) return;
    if (ok && holder) {
      // The station refuses a keyed stream for a few seconds at a change of keyed show (until the
      // last show's holders have heard it to its end) and at its window's lead: the page asks the
      // address itself what it says, and waits quietly while it says that, up to a minute.
      for (let i = 0; i < 20 && listening && holder && !keyCut; i++) {
        const said = await keyedRoutes.streamRefusal(choice?.url ?? '');
        if (!said || !keyedRoutes.WAIT_CODES.includes(said)) break;
        await new Promise((r) => setTimeout(r, 3_000));
      }
    }
    if (ok && holder && Date.now() - lastRestart < 5_000 && ++keyedRetries > 3) {
      playing = listening = false;
      streamProblem = `The stream stopped. Press ${playLabel(choice?.mode)} to try again.`;
      return;
    }
    if (Date.now() - lastRestart >= 5_000) keyedRetries = 0;
    if (keyCut || !listening) return;
    await restart();
  }

  function pauseAll() {
    listening = false;
    audio?.pause();
    screen?.pause();
    stopHls();
  }

  /** A key typed or pasted at `/listen/` was accepted. The press that sent it was the
   *  listener's own, so the show it opens starts now, as it would from Listen. A key for a show
   *  that is not on starts nothing: its panel says when its show is. A browser that holds the
   *  sound back all the same (the station took too long to answer for the press to count) is no
   *  fault: the page says what to press. */
  async function keyAccepted() {
    await tick();
    if (!keyPage || view !== 'listen' || !holder || playing || !choice?.url) return;
    keyCut = false;
    streamProblem = null;
    try {
      await start();
    } catch {
      // Nothing is left loading unheard: Listen starts it afresh.
      pauseAll();
      pressNote = true;
    }
  }

  async function toggle() {
    if (!choice || !choice.url) return;
    streamProblem = null;
    pressNote = false;
    if (playing) {
      pauseAll();
      return;
    }
    keyCut = false;
    try {
      await start();
    } catch {
      streamProblem = "The stream didn't start. Try again in a moment.";
    }
  }

  /** Another stream (ADR-0232): kept for next time by its public address, and played at once
   *  if the stream plays (as the keyed stream of the same kind, for a key's holder). */
  function chooseStream(url: string) {
    // A stream that plays starts again on the new choice (the effect on `playKey`).
    chosenStream = url;
    streams.remember(url);
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
    if (insecure) return;
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
      } else if (closedUnderForm(err)) {
        // The lines closed while the form was open (ADR-0271): back to the player, which now
        // says so, with the one sentence that explains what happened to the form.
        await load();
        view = 'listen';
        sendProblem = CLOSED_WHILE_FILLING;
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
    useStationIce(info?.relay);
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
      <h1 id="show-title">{keyedShow?.name || info?.show || info?.station || 'Loading…'}</h1>
      {#if info?.presenter}
        <p class="muted">with {info.presenter}{info.presenter_live ? ', live now' : ''}</p>
      {/if}
      {#if keyedShow || k.session}
        <!-- A keyed show says so, and a key's holder sees that their key is in use here: quiet,
             never a lamp, and never the key's name, which is staff's own. -->
        <p class="keyline" data-testid="keyed-line">
          {#if keyedShow}<span class="kchip" data-keyed-show><svg aria-hidden="true"><use href="#g-key" /></svg>For people with a key</span>{/if}
          {#if k.session}<span class="kchip held" data-testid="keyed-badge"><svg aria-hidden="true"><use href="#g-key" /></svg>{BADGE}</span>{/if}
        </p>
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
          >{playing ? 'Pause' : playLabel(choice?.mode)}</button
        >
        {#if choice?.canToggle}
          <button type="button" class="btn quiet wide" aria-pressed={audioOnly} onclick={() => setAudioOnly(!audioOnly)}
            >{audioOnly ? 'Show the picture' : 'Audio only'}</button
          >
        {/if}
        {#if choice?.mode === 'icecast' && (info?.streams ?? []).length > 1}
          <fieldset class="group streams" data-player="streams">
            <legend class="caps">Stream</legend>
            {#each info?.streams ?? [] as s (s.url)}
              {@const why = streams.hint(s, info?.streams ?? [])}
              <label class="check"
                ><input type="radio" name="stream" value={s.url} checked={publicChoice === s.url} onchange={() => chooseStream(s.url)} data-codec={s.codec} data-kbps={s.kbps} /><span
                  >{streams.label(s)}{#if why}<span class="small hint"> · {why}</span>{/if}</span
                ></label
              >
            {/each}
          </fieldset>
        {/if}
        {#if choice?.note}<p class="small" role="status">{choice.note}</p>{/if}
        {#if info && !choice?.url}<p class="small">The stream is not set up at this station yet.</p>{/if}
        {#if streamProblem}<p class="problem" role="alert">{streamProblem}</p>{/if}
        {#if pressNote && !playing && holder}<p class="small" role="status" data-testid="key-press">{pressToHear(playLabel(choice?.mode), keyedShow?.name ?? null)}</p>{/if}
        {#if keyCut}<p class="problem" role="alert" data-testid="key-cut">The show has stopped here because your key stopped working. The reason is below.</p>{/if}
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
      <audio bind:this={audio} preload="none" onplay={() => (playing = true)} onpause={() => (playing = false)} onerror={audioStopped} onended={audioStopped}></audio>
    </section>

    <!-- A listener's key (1.3.2), under the show it is for and above the call: while a show needs
         a key, this is what someone listening wants first. -->
    {#if view === 'listen'}
      <KeyPanel {k} station={info?.station ?? ''} {keyedShow} onAir={live} {playing} streamFailed={!!streamProblem} entry={keyPage && keyAsked} onaccepted={keyAccepted} />
    {/if}

    {#if view === 'call' && call}
      <CallPanel {call} relayPort={info?.relay.turn_tls_port ?? 5349} onclose={backToShow} />
    {:else if view === 'lobby' && info}
      <form class="card lobby" novalidate onsubmit={submit} aria-labelledby="lobby-h">
        <h2 id="lobby-h" tabindex="-1" bind:this={lobbyHeading}>Call the studio</h2>
        <p class="small">The studio answers first. You're not on air until the presenter brings you in, and this page tells you when you are.</p>

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
          <span class="hint" id="topic-hint">One line for the studio.</span>
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
        {#if insecure}
          <!-- No Call the studio button here: it could never work on this page (design review F6). -->
          <p class="small" data-needs-https>{NEEDS_HTTPS}</p>
        {:else if kept}
          <p>You were on a call as <b>{kept.name}</b>. Your place in the queue is held for a few minutes.</p>
          <button type="button" class="btn primary big wide" onclick={rejoin}>Rejoin your call</button>
          <button type="button" class="btn quiet wide" onclick={() => { store.forget(); kept = null; }}>Forget it</button>
        {:else if info.call_in.open}
          <p class="small">Call from this page with your phone or computer's microphone. No phone number needed.</p>
          <button type="button" class="btn primary big wide" onclick={openLobby}
            ><svg aria-hidden="true"><use href="#g-mic" /></svg>Call the studio</button
          >
        {:else}
          <p class="small" role="status">{closedLine(info.call_in.why_not, chatOpen)}</p>
          <button type="button" class="btn big wide" disabled>Call the studio</button>
        {/if}
        {#if sendProblem && view === 'listen'}<p class="problem" role="alert">{sendProblem}</p>{/if}
      </section>
    {/if}

    <!-- The chat stays mounted through the lobby and a call, so it keeps its place and session. -->
    <ChatPanel {playing} inCall={view === 'call'} oninvite={() => view !== 'call' && openLobby()} onfeatured={(m) => (onAir = m)} onopen={(open) => (chatOpen = open)} />
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
  .keyline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s-2);
  }
  .kchip {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    min-height: 1.75rem;
    padding: 0 var(--s-2);
    border: 1px solid var(--axis);
    border-radius: var(--r-pill);
    font-size: var(--fs-14);
    color: var(--ink-2);
    line-height: 1;
  }
  .kchip svg {
    width: 14px;
    height: 14px;
    flex: none;
  }
  .kchip.held {
    color: var(--ink);
    border-color: var(--ink-2);
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
