<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // The join page for people the station invites: a guest, a remote DJ, or a presenter in a
  // browser. The invite is a pass in the link's fragment (`/join/#rsp1...`), which the browser
  // never sends to a server. The page says before joining whether they go on air at once (a
  // guest or presenter) or wait for the handover (a DJ), checks the microphone, then joins.
  import { onDestroy, onMount } from 'svelte';
  import Glyphs from '../../studio/src/lib/Glyphs.svelte';
  import BrandMark from './lib/BrandMark.svelte';
  import * as api from './lib/api.ts';
  import type { Call } from './lib/call.svelte.ts';
  import CallPanel from './lib/CallPanel.svelte';
  import CameraCheck from './lib/CameraCheck.svelte';
  import MicCheck from './lib/MicCheck.svelte';
  import { CameraPreview } from './lib/camera-preview.svelte.ts';
  import { wantsCameraPreview } from './lib/rules.ts';
  import { useTurnPort } from './lib/turn.ts';

  const pass = (globalThis.location?.hash ?? '').replace(/^#/, '').trim();
  let info = $state<api.PassInfo | null>(null);
  let problem = $state<string | null>(pass ? null : 'This page needs the link the station sent you. Open that link, all of it, on this device.');
  let call = $state<Call | null>(null);
  let joining = $state(false);
  let relay = $state(false);
  // The person's own picture before they enter (ADR-0171), for the people who appear on the
  // show while the station has video on. Nothing is sent until they press join.
  const preview = new CameraPreview();
  let previewing = $state(false);
  // Join waits until the page knows whether to show the preview: a press before that took the
  // camera at join and then opened a second capture for the preview (review 0028, C3).
  let checkingVideo = $state(true);

  onMount(async () => {
    if (!pass) return;
    try {
      info = await api.passInfo(pass);
      document.title = `Join ${info.station}`;
      if (info.ended) problem = 'The studio has ended this invitation. Ask the station for a new link.';
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    }
    await prepare();
  });

  /** Once the pass can join: the camera's preview, where the station has video on. Not while the
   *  show's step keeps the seat off (ADR-0271): nobody is asked for a camera they cannot use yet. */
  async function prepare() {
    if (info && !info.ended && !info.not_now && !previewing) {
      checkingVideo = true;
      // The player's public figures say whether video is on; unknown means no preview.
      const videoOn = (await api.player().catch(() => null))?.video != null;
      if (wantsCameraPreview(info.role, videoOn)) {
        previewing = true;
        void preview.start();
      }
    }
    checkingVideo = false;
  }
  onDestroy(() => preview.stop());

  const as: Record<string, string> = { guest: 'as a guest', dj: 'as a remote DJ', presenter: 'to present' };
  const what: Record<string, string> = {
    guest: "You're on air as soon as you join, so join when you're ready to talk. The studio can take you off.",
    presenter: "You're on air as soon as you join. Your page says so in red.",
    dj: "You join off air and hear the show. You go on air when the presenter hands over to you; your page says when.",
  };
  const until = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  // The show on air may run at a step that has switched this seat off (ADR-0271): the page says
  // so in the station's sentence, and the link stays good for later, so it can look again.
  let looking = $state(false);
  async function lookAgain() {
    looking = true;
    try {
      info = await api.passInfo(pass);
      if (info.ended) problem = 'The studio has ended this invitation. Ask the station for a new link.';
      await prepare();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      looking = false;
    }
  }

  async function join() {
    joining = true;
    problem = null;
    try {
      const joined = await api.passToken(pass);
      const { Call } = await import('./lib/call.svelte.ts');
      if (info) useTurnPort(info.relay.turn_tls_port);
      call = new Call(joined, relay, previewing ? preview.handOver() : null);
      void call.start();
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      joining = false;
    }
  }
</script>

<Glyphs />
<div class="page">
  <header class="top">
    <BrandMark />
    <span class="station">{info?.station ?? 'WeaveSignal'}</span>
  </header>

  <main class="main" aria-label="Join the studio">
    {#if call && info}
      <h1 class="sr-only">{info.station}: {info.name || 'you'} {as[info.role] ?? ''}</h1>
      <CallPanel {call} relayPort={info.relay.turn_tls_port} />
    {:else}
      <section class="card" aria-labelledby="join-h">
        {#if info}
          <span class="caps">Invitation</span>
          <h1 id="join-h">Join {info.show || info.station} {as[info.role] ?? ''}</h1>
          {#if info.name}<p>Joining as <b>{info.name}</b></p>{/if}
          <!-- What joining does is said once the seat is open: not beside "Not just now". -->
          {#if !info.not_now}<p class="small">{what[info.role] ?? ''}</p>{/if}
          <p class="small">This link works until {until(info.expires_at_ms)}, and only for you: don't share it.</p>
        {:else if !problem}
          <h1 id="join-h">Opening your invitation…</h1>
        {:else}
          <h1 id="join-h">This link doesn't work</h1>
        {/if}
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
      </section>

      {#if info && !info.ended && info.not_now}
        <section class="card" aria-labelledby="notnow-h">
          <h2 id="notnow-h">Not just now</h2>
          <p role="status">{info.not_now}</p>
          <button type="button" class="btn wide" disabled={looking} aria-busy={looking || undefined} onclick={lookAgain}>{looking ? 'Looking…' : 'Look again'}</button>
        </section>
      {:else if info && !info.ended}
        <section class="card" aria-label="Before you join">
          <MicCheck />
          {#if previewing}<CameraCheck {preview} />{/if}
          <p class="note"><svg aria-hidden="true"><use href="#g-mic" /></svg><span>Wear headphones if you can: the studio's sound through speakers comes back through your microphone as an echo.</span></p>
          <label class="check">
            <input type="checkbox" bind:checked={relay} />
            <span class="relay">Connect through the station's relay <span class="small">(for networks and browsers that block calls; port {info.relay.turn_tls_port})</span></span>
          </label>
          <button type="button" class="btn primary big wide" disabled={joining || checkingVideo || preview.starting} onclick={join}>{joining ? 'Joining…' : 'Join the studio'}</button>
        </section>
      {/if}
    {/if}
  </main>
</div>

<style>
  .main {
    display: grid;
    gap: var(--s-4);
  }
  h1 {
    font-size: var(--fs-24);
  }
</style>
