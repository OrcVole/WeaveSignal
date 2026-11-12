<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The echo test: the only page that turns echo cancelling off (packages/ui/media.ts). It plays
  // your microphone back to you as it is, half a second late, so you hear what the room would
  // hear without the join pages' echo cancelling: your voice, your room, and any sound your
  // speakers leak into the microphone. It joins nothing; nothing leaves this device.
  import { onDestroy } from 'svelte';
  import { ECHO_TEST_CAPTURE } from '../../../packages/ui/media.ts';
  import Glyphs from '../../studio/src/lib/Glyphs.svelte';
  import BrandMark from './lib/BrandMark.svelte';
  import { micProblem } from './lib/mic.ts';
  import { Meter } from './lib/meter.svelte.ts';

  let stream = $state<MediaStream | null>(null);
  let meter = $state<Meter | null>(null);
  let ctx: AudioContext | null = null;
  let problem = $state<string | null>(null);

  async function start() {
    problem = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: { ...ECHO_TEST_CAPTURE } });
      ctx = new AudioContext();
      const src = ctx.createMediaStreamSource(stream);
      const delay = ctx.createDelay(1);
      delay.delayTime.value = 0.5;
      src.connect(delay).connect(ctx.destination);
      meter = new Meter(stream.getAudioTracks()[0]);
    } catch (e) {
      problem = micProblem(e);
    }
  }
  function stop() {
    meter?.stop();
    meter = null;
    void ctx?.close().catch(() => {});
    ctx = null;
    for (const t of stream?.getTracks() ?? []) t.stop();
    stream = null;
  }
  onDestroy(stop);
</script>

<Glyphs />
<div class="page">
  <header class="top"><BrandMark /><span class="station">Echo test</span></header>
  <main class="card" aria-labelledby="echo-h">
    <h1 id="echo-h">Hear your microphone as it is</h1>
    <p class="small">
      This page plays your microphone back to you half a second late, <b>with echo cancelling off</b>. It's the only page that turns it off. Nothing
      leaves this device.
    </p>
    <p class="small">Wear headphones: through speakers you'll hear a loop. If you hear the show or another voice in your own microphone, use headphones when you join.</p>
    {#if stream}
      <span class="level" role="img" aria-label={meter?.heard ? 'Your microphone is working' : 'Not heard yet'}><i style:width="{meter?.pct ?? 0}%"></i></span>
      <p class="small" role="status">Listening. Speak and you'll hear yourself.</p>
      <button type="button" class="btn wide" onclick={stop}>Stop</button>
    {:else}
      <button type="button" class="btn primary big wide" onclick={start}>Start the echo test</button>
    {/if}
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  </main>
</div>

<style>
  h1 {
    font-size: var(--fs-24);
  }
</style>
