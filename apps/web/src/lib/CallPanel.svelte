<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The in-call panel every join page shares: where you stand (one banner, announced as text),
  // whether the show is on air and the presenter live, what you hear, your microphone's level,
  // the relay for networks that block calls, and hanging up (two presses, like the desk's drop).
  import { onDestroy } from 'svelte';
  import type { Call } from './call.svelte.ts';
  import { Meter } from './meter.svelte.ts';
  import { banner, showLine } from './words.ts';

  let { call, relayPort, onclose }: { call: Call; relayPort: number; onclose?: () => void } = $props();

  let now = $state(Date.now());
  const clock = setInterval(() => (now = Date.now()), 500);
  onDestroy(() => clearInterval(clock));

  const b = $derived(banner(call.role, call.me, call.link, now, call.heldUntil));
  const filled = $derived(b.tone === 'onair' || b.tone === 'open' || b.tone === 'cue' || b.tone === 'talk');
  const line = $derived(showLine(call.me));
  const live = $derived(call.me?.show_on_air ?? false);
  const ended = $derived(call.link === 'ended' || call.link === 'lost');
  // The microphone was refused before the page joined: say how to allow it, and try again.
  const noMic = $derived(call.link === 'no-mic');

  let meter = $state<Meter | null>(null);
  $effect(() => {
    const t = call.mic;
    const m = t ? new Meter(t) : null;
    meter = m;
    return () => m?.stop();
  });

  // The desk's picture (ADR-0028) and our own camera's preview, while video is on.
  let screen = $state<HTMLVideoElement | undefined>();
  let preview = $state<HTMLVideoElement | undefined>();
  $effect(() => {
    if (screen) screen.srcObject = call.picture ? new MediaStream([call.picture]) : null;
  });
  $effect(() => {
    if (preview) preview.srcObject = call.camera ? new MediaStream([call.camera]) : null;
  });

  let armed = $state(false);
  let armTimer: ReturnType<typeof setTimeout> | undefined;
  function hangUp() {
    if (!armed) {
      armed = true;
      clearTimeout(armTimer);
      armTimer = setTimeout(() => (armed = false), 3000);
      return;
    }
    clearTimeout(armTimer);
    armed = false;
    void call.hangUp();
  }
  const leaveWord = $derived(call.role === 'caller' ? 'Hang up' : 'Leave the studio');
</script>

<section class="call" aria-labelledby="call-headline" data-role={call.role}>
  <div class="banner tone-{b.tone}" class:filled role="status" aria-live="polite" aria-atomic="true" data-phase={call.me?.phase ?? 'none'} data-link={call.link}>
    <span class="state {b.chip.cls}"><svg class={b.chip.glyphCls} aria-hidden="true"><use href="#{b.chip.glyph}" /></svg>{b.chip.word}</span>
    <h2 id="call-headline">{b.headline}</h2>
    <p>{b.detail}</p>
  </div>

  {#if call.me?.talkback && !ended}
    <p class="note talkback" role="status"><svg aria-hidden="true"><use href="#g-talk" /></svg><span><b>Talkback:</b> the studio is talking in your ear. Listeners can't hear it.</span></p>
  {/if}

  {#if line && !ended && !noMic}
    <p class="showline" data-show-on-air={live}>
      <svg aria-hidden="true"><use href={live ? '#g-onair' : '#g-standby'} /></svg>{line}
    </p>
  {/if}

  {#if call.needsTap && !ended}
    <button type="button" class="btn primary big wide" onclick={() => call.tap()}>Tap to hear the studio</button>
  {/if}

  {#if !ended && (call.picture || call.camera)}
    <div class="picture card" data-panel="join.picture">
      <span class="caps">The show's picture</span>
      <!-- The studio's sound comes through the call; the picture carries none. -->
      <video bind:this={screen} class="screen" autoplay muted playsinline aria-label="What the show's viewers see, live"></video>
      {#if !call.picture}<p class="small">The studio's picture appears here.</p>{/if}
      {#if call.camera}
        <div class="row">
          <video bind:this={preview} class="preview" class:off={!call.cameraOn} autoplay muted playsinline aria-label="Your camera"></video>
          <button type="button" class="btn" aria-pressed={!call.cameraOn} onclick={() => call.setCamera(!call.cameraOn)}
            >{call.cameraOn ? 'Turn your camera off' : 'Turn your camera on'}</button
          >
        </div>
      {/if}
    </div>
  {/if}
  {#if call.cameraProblem && !ended}<p class="small" role="status">{call.cameraProblem}</p>{/if}

  {#if !ended && !noMic}
    <div class="io card">
      <div class="row">
        <span class="caps">You hear</span>
        <span data-hearing={call.hearing}>{call.hearing ? 'The show and the studio, through this page' : 'Nothing yet'}</span>
      </div>
      <div class="row">
        <span class="caps">Your microphone</span>
        <span class="level" role="img" aria-label={meter?.heard ? 'Your microphone is working' : 'Your microphone has not been heard yet'}><i style:width="{meter?.pct ?? 0}%"></i></span>
      </div>
      <p class="small">Turn any radio or other player off: this page already plays the show. Headphones stop the show echoing back.</p>
    </div>
  {/if}

  {#if call.problem}
    <p class="problem" role="alert">{call.problem}</p>
  {/if}

  {#if noMic}
    <button type="button" class="btn primary big wide" onclick={() => call.start()}>Try again</button>
  {/if}

  {#if ended}
    {#if onclose}<button type="button" class="btn primary wide" onclick={onclose}>Back to the show</button>{/if}
  {:else}
    <button type="button" class="btn danger-hollow wide" class:armed onclick={hangUp} aria-describedby={armed ? 'hangup-armed' : undefined}
      >{armed ? `Press again to ${leaveWord.toLowerCase()}` : leaveWord}</button
    >
    {#if armed}<p id="hangup-armed" class="small" role="status">{call.role === 'caller' ? 'Your place in the queue goes when you hang up.' : 'You can rejoin from your invite link while it lasts.'}</p>{/if}

    <details class="trouble">
      <summary>Trouble hearing or being heard?</summary>
      <div>
        <p class="small">Studio too quiet? On a phone, press volume up once the call has started: a call has its own volume, apart from music's.</p>
        {#if call.relay}
          <p class="small">You're connected through the station's relay (TURN over TLS, port {relayPort}).</p>
        {:else}
          <p class="small">
            Some networks and browsers (GrapheneOS's, many workplaces') block calls. The station's relay gets through them, over port {relayPort}.
          </p>
          <button type="button" class="btn wide" onclick={() => call.useRelay()}>Reconnect through the relay</button>
        {/if}
        <p class="small">Can't hear yourself echoing? That's right: this page cancels echo. To hear your own microphone as it is, use the <a href="/echo-test/" target="_blank" rel="noopener">echo test</a>.</p>
      </div>
    </details>
  {/if}
</section>

<style>
  .picture {
    display: grid;
    gap: var(--s-2);
  }
  .picture .screen {
    width: 100%;
    aspect-ratio: 16 / 9;
    background: #000;
    border-radius: 6px;
  }
  .picture .preview {
    width: 128px;
    aspect-ratio: 16 / 9;
    background: #000;
    border-radius: 4px;
  }
  .picture .preview.off {
    opacity: 0.3;
  }
  .call {
    display: grid;
    gap: var(--s-3);
  }
  .banner {
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    padding: var(--s-4);
    border-radius: var(--r-3);
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-left: 6px solid var(--axis);
    box-shadow: var(--e-1);
  }
  .banner h2 {
    font-size: var(--fs-24);
  }
  .banner p {
    color: var(--ink-2);
    line-height: var(--lh-label);
  }
  .banner.tone-good {
    border-left-color: var(--status-good);
  }
  .banner.tone-warn {
    border-left-color: var(--status-warning);
  }
  .banner.tone-ended {
    border-left-color: var(--muted);
  }
  .banner.filled {
    border-color: transparent;
  }
  .banner.filled p {
    color: inherit;
  }
  .banner.tone-onair {
    background: var(--live-onair);
    color: var(--live-onair-ink);
  }
  .banner.tone-onair h2 {
    font-size: var(--fs-32);
  }
  .banner.tone-open {
    background: var(--live-open);
    color: var(--live-open-ink);
  }
  .banner.tone-cue {
    background: var(--live-cue);
    color: var(--live-cue-ink);
  }
  .banner.tone-talk {
    background: var(--live-talk);
    color: var(--live-talk-ink);
  }
  .banner.filled .state {
    background: transparent;
    border-color: currentColor;
    color: inherit;
  }
  .banner.filled .state svg {
    color: inherit;
  }
  .showline {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font-weight: 600;
  }
  .showline svg {
    width: 16px;
    height: 16px;
    flex: none;
    color: var(--muted);
  }
  .talkback {
    border-left: 3px solid var(--live-talk);
    padding: var(--s-2) var(--s-3);
    background: var(--surface-2);
    border-radius: var(--r-1);
  }
  .talkback svg {
    color: var(--live-talk);
  }
  .io .row {
    display: grid;
    gap: var(--s-1);
  }
  .io .level {
    display: block;
  }
</style>
