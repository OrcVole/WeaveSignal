<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Desk B, the focus (ADR-0061), wired to the desk through the control plane: time on the
  // left (clock, rundown, playout), what is on air in the middle (the lamp, the stage, the
  // sources not on air), the station on the right, and the station's health along the foot.
  // Panel names as on the design page (ADR-0070); the toast takes the chat glance's cell.
  //
  // The show's step decides what is drawn (ADR-0271; docs/reference/design/beginner-steps.md,
  // section 1): nothing moves, things only appear. Solo is one tile, the guest's empty seat and
  // no Not on air row; Guests brings that row and Invite someone…; Callers brings the delay
  // with Dump and the queue on the desk (`desk.callers`); Full is the studio of 1.0.1, with the
  // picture, the handover and the Screener button. While everything is shown (ADR-0274) the
  // parts the step keeps off carry the dashed tag and their controls are disabled.
  //
  // While this window is preparing (ADR-0276) the desk is the live show to follow: every panel
  // stays where it is and keeps its states, and every control that changes the air is disabled,
  // with the reason in its tooltip. Invitations and the step stay, as the station leaves them.
  import { onMount } from 'svelte';
  import Tile from './Tile.svelte';
  import AirControl from './AirControl.svelte';
  import Cameras from './Cameras.svelte';
  import SlateStill from './SlateStill.svelte';
  import type { Notice } from '../lib/api.ts';
  import { CHIP, isOpen, listenersHear, masterLamp } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { look, studio, stationNow } from '../lib/studio.svelte.ts';
  import { run, readyCaller } from '../lib/actions.ts';
  import { account } from '../lib/account.svelte.ts';
  import { sandboxed } from '../lib/api.ts';
  import { chat, startChat } from '../lib/chat.svelte.ts';
  import {
    STEP_WORD,
    chatNeeds,
    chatNeedsWhere,
    deskCallers,
    deskCallersShown,
    needsOwnLink,
    offHere,
    offTag,
    shown,
    stageOf,
    stationStep,
    stepNoteShown,
    type Part,
  } from '../lib/steps.ts';
  import { atToday, delayParts, delayWords, dur, firstName, hm, hms, listenerDelayMs } from '../lib/format.ts';
  import { bankAt, bankOwner, cartAt, cartNow } from '../../../../packages/ui/carts.ts';
  import { audienceWords, cdnWarning } from '../lib/audience.ts';
  import { HANDOVER_KEY, handoverButton, handoverStep } from '../lib/handover.ts';
  import Invite from './Invite.svelte';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';

  const v = $derived(studio.view);
  /** The station's step: what is switched on. */
  const step = $derived(stationStep(v));
  /** What this window draws: the step, or everything while Show everything is on. */
  const lk = $derived(look());
  /** Drawn because everything is shown, and kept off by the step: the dashed tag, a disabled control. */
  const off = (part: Part) => offHere(part, step, lk);
  const tag = $derived(offTag(step));
  /** This window is preparing (ADR-0276): it shows the live desk and drives none of it. */
  const prep = $derived(preparing());
  const prepWhy = $derived(prep ? OFF_WHY : undefined);
  /** The desk's picture (M5); null with video off. */
  const pic = $derived(v?.desk.picture ?? null);
  /** More than about 500 connected with video on and no CDN (ADR-0017). */
  const cdn = $derived(cdnWarning(v));
  const audience = $derived(audienceWords(v));
  const shotName = $derived.by(() => {
    const id = pic?.identity;
    if (!id) return '';
    const s = v?.sources.find((x) => x.identity === id);
    return s ? firstName(s.name, id) : id;
  });
  /** The listener delay: read live where it can be, the setting for the rest (ADR-0103, ADR-0272). */
  const LISTENER_DELAY_MS = $derived(listenerDelayMs(v?.listener_delay));
  const behind = $derived(delayWords(v?.listener_delay));
  /** What of the estimate is read live and what is a setting (ADR-0272). */
  const delayDetail = $derived(delayParts(v?.listener_delay));
  const now = $derived(stationNow());
  const playout = $derived(v?.desk.playout ?? { connected: false, for_ms: null });
  const up = $derived(!!v?.desk.reachable && playout.connected);
  /** The station's mode (ADR-0183): `auto` means automation holds the air and the desk never airs. */
  const mode = $derived(v?.mode ?? null);
  const auto = $derived(mode === 'auto');
  /** Whether listeners hear the room: playout is connected and automation does not hold the air. */
  const heard = $derived(up && listenersHear(playout.connected, mode));
  const sources = $derived(v?.sources ?? []);
  // At Solo nothing moves: every tile stays on the stage and the Not on air row is not drawn.
  const placed = $derived(stageOf(sources, lk));
  const onStage = $derived(placed.stage);
  const offStage = $derived(placed.waiting);
  /** The Solo stage offers the DJ their own microphone link until a presenter is in the room. */
  // Not in a preparing window: that link joins on air, which is not a preparing window's to do.
  const ownLink = $derived(!prep && !!v?.desk.reachable && needsOwnLink(sources, lk, !!v?.step?.may_change, !account.me || account.me.kind === 'staff'));
  /** The callers waiting on the desk, at the Callers step. */
  const queue = $derived(deskCallers(v));
  const openCount = $derived(sources.filter(isOpen).length);
  /** The handover button (ADR-0074): what ⇧⏎ does next, worked out once for the DJ's tile and
   *  the rundown's row, so the key and its buttons never differ. Null with nothing to press. */
  const handover = $derived(v && shown('handover', lk) ? handoverButton(handoverStep(sources, v.seats.presenter, studio.handoverTo)) : null);
  const lamp = $derived(
    masterLamp({ deskReachable: !!v?.desk.reachable, playoutConnected: playout.connected, anyOnAir: openCount > 0, mode }),
  );
  const peopleIn = $derived(sources.filter((s) => s.present).length + (v?.callers.filter((c) => c.present && c.slot === null).length ?? 0));

  // ---- the cart glance: the current bank's top row, F1 to F6 (the wall's 1 to 6) ----
  const glanceBank = $derived(bankAt(studio.catalogue, studio.bank));
  const glance = $derived(
    [1, 2, 3, 4, 5, 6].map((n) => {
      const cart = cartAt(studio.catalogue, studio.bank, n);
      return { n, cart, now: cartNow(cart, v?.carts ?? null, now) };
    }),
  );
  const RING_C = 2 * Math.PI * 9;

  // ---- the clock ----
  const tenth = $derived(Math.floor((now % 1000) / 100));
  const rundown = $derived(
    (v?.show.rundown ?? []).map((r) => ({ ...r, ms: atToday(r.at, now, studio.zone) })).sort((a, b) => a.ms - b.ms),
  );
  const post = $derived(rundown.find((r) => r.hard_post && r.ms > now) ?? null);
  const postFrom = $derived.by(() => {
    if (!post) return 0;
    const before = rundown.filter((r) => r.ms < post.ms && r.ms <= now).at(-1);
    return before?.ms ?? post.ms - 10 * 60_000;
  });
  const left = $derived(post ? post.ms - now : 0);
  const coming = $derived(rundown.filter((r, i) => r.ms > now || rundown[i + 1]?.ms > now).slice(0, 5));
  const current = $derived(rundown.filter((r) => r.ms <= now).at(-1) ?? null);

  // ---- the toast slot: one toast, most urgent first ----
  type Toast = {
    /** `plain`: a note with no tone colour (a hidden part's key). */
    tone: 'good' | 'warning' | 'serious' | 'plain';
    glyph: string;
    title: string;
    msg: string;
    /** `air`: the action changes what is on air, so a preparing window does not offer it. */
    acts: { label: string; keys: string[]; primary?: boolean; air?: boolean; go: () => void }[];
  };
  const caller = $derived(readyCaller());
  const degradedOnAir = $derived(sources.find((s) => s.on_air && (!s.present || !s.linked)) ?? null);
  const lastNotice: Notice | null = $derived(
    v?.notices
      .filter((n) => n.surface === 'desk' && !studio.dismissed.includes(n.id) && now - n.at_ms < 20_000 && n.kind !== 'caller.ready')
      .at(-1) ?? null,
  );
  const anyToast: Toast | null = $derived.by(() => {
    if (studio.offNote) {
      return {
        tone: 'plain',
        glyph: 'g-standby',
        title: studio.offNote.title,
        msg: studio.offNote.message,
        acts: [
          { label: 'Step…', keys: [], go: () => ((studio.offNote = null), (studio.stepMenu = true)) },
          { label: 'Dismiss', keys: ['Esc'], go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (studio.note) {
      return { tone: studio.note.tone, glyph: 'g-good', title: studio.note.title, msg: studio.note.message, acts: [] };
    }
    if (v?.desk.reachable && !playout.connected) {
      return {
        tone: 'serious',
        glyph: 'g-playout',
        title: 'Playout down · room alive',
        msg: `${playout.for_ms === null ? 'Liquidsoap has not connected to the desk.' : `SRT to Liquidsoap dropped ${dur(playout.for_ms)} ago.`} Listeners hear nothing of the room; the room keeps talking and its sources say OPEN.`,
        acts: [{ label: 'Restart playout', keys: ['⌃R'], primary: true, air: true, go: () => run({ type: 'restart-playout' }) }],
      };
    }
    if (caller) {
      const n = caller.caller?.name_on_air || firstName(caller.name, caller.identity);
      const k = slotKeys(caller.slot);
      return {
        tone: 'good',
        glyph: 'g-phone',
        title: `${n} is ready for you · strip ${caller.slot}`,
        msg: [caller.caller?.line, caller.caller?.note].filter(Boolean).join(' · ') || 'Sent by the screener.',
        acts: [
          { label: `Cue ${n}`, keys: [k.cue], primary: true, air: true, go: () => run({ type: 'cue', slot: caller.slot }) },
          { label: 'Not now', keys: ['Esc'], air: true, go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (degradedOnAir) {
      const n = firstName(degradedOnAir.name, degradedOnAir.identity);
      const k = slotKeys(degradedOnAir.slot);
      return {
        tone: 'warning',
        glyph: 'g-link-degraded',
        title: degradedOnAir.present ? `${n}'s link is degraded: no audio reaching the desk` : `${n} left the room`,
        msg: degradedOnAir.present
          ? 'Mute them and cue them back in when their audio returns.'
          : 'Their place on the desk is kept for 30 s; if they rejoin they are mixed again.',
        acts: [
          { label: `Mute ${n}`, keys: [k.mute], air: true, go: () => run({ type: 'mute', slot: degradedOnAir.slot }) },
          { label: 'Keep them on', keys: ['Esc'], go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (lastNotice) return { tone: lastNotice.tone, glyph: 'g-good', title: lastNotice.title, msg: lastNotice.message, acts: [] };
    return null;
  });
  const toast: Toast | null = $derived.by(() => {
    const t = anyToast;
    // A preparing window reads the toast and is offered none of its on-air answers.
    return t && prep ? { ...t, acts: t.acts.some((a) => a.air) ? [] : t.acts } : t;
  });

  const sub = $derived.by(() => {
    if (!v) return '';
    if (!v.desk.reachable) return `The desk is not answering: ${v.desk.error ?? 'no reason given'}`;
    return '';
  });
  const dumpArmed = $derived(studio.armed?.what === 'dump');
  const delay = $derived(v?.desk.delay ?? null);
  /** The dump button is live whenever the delay holds anything, even while it rebuilds (ADR-0166). */
  const dumpable = $derived(!!delay && delay.target_ms > 0 && delay.delay_ms > 0);
  const secs = (ms: number) => (ms / 1000).toFixed(1);
  const screenerSeat = $derived(v?.seats.screener ?? null);
  const screenerName = $derived(screenerSeat ? firstName(v?.seats.screener_name ?? '', screenerSeat) : null);

  // ---- the chat glance: the room's last messages, to read, at every step ----
  // The chat's link starts with the desk now (at Solo there is no Screener tab to start it).
  // The sandbox has no chat service, so it says so instead of trying.
  onMount(() => {
    if (!sandboxed()) startChat();
  });
  const CHAT_LINES = 8;
  const chatLines = $derived(chat.room.lines.filter((m) => !chat.room.hidden[m.id] && m.state === 'visible').slice(-CHAT_LINES));
  /** What waits for a moderator, as the station counts it (ADR-0274). */
  const needs = $derived(chatNeeds(v?.chat));
  /** The newest message to the studio, private to the screener and the presenter: Full's line. */
  const priv = $derived(step === 'full' ? (chat.mod.studio.at(-1)?.m ?? null) : null);
  const chatSays = $derived.by(() => {
    if (sandboxed()) return 'The live chat is not in the sandbox; on a real station listeners chat from the player page.';
    if (chat.link === 'none') return 'This station has no chat.';
    if (chat.link === 'lost') return `The chat is not connected${chat.why ? `: ${chat.why}` : ''}. What shows may be out of date.`;
    if (chat.link !== 'live') return 'Connecting to the chat…';
    return chatLines.length ? '' : 'Nobody has written yet.';
  });
</script>

<div class="desk">
  <div class="cols">
    <!-- left: time -->
    <div class="col left">
      <section class="panel clock" data-panel="desk.clock" aria-label="Studio clock">
        <div class="time num" data-panel-title>{hms(now)}<small>.{tenth}</small></div>
        {#if post}
          <div class="post">
            <span class="name">{post.title} · hard post {post.at.length === 5 ? `${post.at}:00` : post.at}</span><span
              class="left num"
              class:soon={left < 10_000}>−{dur(left)}</span
            >
            <div class="bar"><i style:width="{Math.min(100, ((now - postFrom) / (post.ms - postFrom)) * 100)}%"></i></div>
          </div>
        {:else}
          <div class="post"><span class="name">No hard post ahead</span></div>
        {/if}
        <div class="listen" title={delayDetail || undefined}>
          <svg aria-hidden="true" style="width:12px;height:12px;vertical-align:-1px"><use href="#g-delay" /></svg>
          Listeners hear <b class="num">{hms(now - LISTENER_DELAY_MS)}</b> · {behind}
          {#if delayDetail}<span class="small" data-testid="delay-detail">{delayDetail}</span>{/if}
        </div>
      </section>

      <section class="panel" data-panel="desk.rundown" aria-label="Rundown">
        <div class="ph">
          <h2>Coming up</h2>
          <span class="spacer"></span><span class="small">{v?.show.title || 'No show set'}</span>
        </div>
        <div class="rundown">
          {#each coming as r (r.at + r.title)}
            <!-- The handover's row (its key is ⇧⏎) carries the button while there is a DJ to press it for. -->
            {@const hand = r.key === HANDOVER_KEY && handover ? handover : null}
            <div class="row" class:now={r === current} class:hand={!!hand}>
              <span class="at num">{hm(r.ms)}</span>
              <span class="what"><b>{r.title}</b>{#if r.detail}<span>{r.detail}</span>{/if}</span>
              {#if hand}
                <!-- The quiet one of the two, outlined (ADR-0240): the DJ's tile carries the accent. -->
                <button type="button" class="btn" data-handover={hand.kind} disabled={prep} title={prepWhy} onclick={() => run({ type: 'handover' })}
                  >{hand.label} <kbd>{HANDOVER_KEY}</kbd></button
                >
              {:else if r === current && openCount > 0}
                <StateChip chip={heard ? CHIP.onAir : CHIP.open} />
              {:else if r.key}
                <span class="small"><kbd>{r.key}</kbd></span>
              {:else}
                <span></span>
              {/if}
            </div>
          {:else}
            <div class="row"><span class="at num">—</span><span class="what"><b>Nothing scheduled</b><span>nothing in <code>studio.rundown</code></span></span><span></span></div>
          {/each}
        </div>
        <div class="small"><span class="proposal">setting</span> rows from <code>studio.rundown</code>; the schedule does not fill this yet</div>
      </section>

      <section class="panel playout" data-panel="desk.playout" aria-label="Playout">
        <div class="ph">
          <h2>Playout</h2>
          <span class="small lg-only">Liquidsoap 2.5</span><span class="spacer"></span>
        </div>
        <div class="plamp">
          {#if !v?.desk.reachable}
            <StateChip chip={CHIP.fault} />
            <span class="small">the desk is not answering</span>
          {:else if playout.connected}
            <!-- In auto mode the log is what listeners hear, so the panel's own chip reads ON AIR (ADR-0183). -->
            <StateChip chip={auto ? CHIP.onAir : CHIP.standby} />
            <span class="small">{auto ? 'Auto · the log · ' : ''}SRT connected {playout.for_ms !== null ? dur(playout.for_ms) : ''}</span>
          {:else}
            <StateChip chip={CHIP.playoutDown} />
            <span class="small"
              >{playout.for_ms === null ? 'never connected' : `SRT link lost ${dur(playout.for_ms)} ago`} · room alive</span
            >
          {/if}
        </div>
        {#if v?.default_fallback}
          <div class="plamp" data-testid="default-loop">
            <StateChip chip={CHIP.defaultLoop} />
            <span class="small">on the stream</span>
          </div>
          <div class="extra warn">
            Listeners hear the loop WeaveSignal ships, because this station has no fallback files of its own. Put some music
            in the station's fallback folder and it takes over within seconds.
          </div>
        {/if}
        <div class="q"><span>Playout's queue and hard posts are not shown here yet.</span></div>
        {#if v?.desk.reachable && !playout.connected}
          <div class="extra">Listeners hear nothing of the room. Restart playout to bring them back.</div>
          <div class="acts">
            <button type="button" class="btn primary" disabled={prep} title={prepWhy} onclick={() => run({ type: 'restart-playout' })}
              >Restart playout <kbd>⌃R</kbd></button
            >
          </div>
        {/if}
      </section>
    </div>

    <!-- centre: the stage -->
    <main class="stage" data-panel="desk.stage" aria-label="On air">
      <header class="stage-head">
        <AirControl {lamp} />
        <div class="title">
          <h1 data-panel-title>{v?.show.title || 'No show set'}</h1>
          {#if sub}
            <div class="sub">{sub}</div>
          {:else if auto}
            <div class="sub"><b>Automation holds the air</b> · {openCount} open to the room · listeners hear the log, {behind}</div>
          {:else if up}
            <div class="sub"><b>{openCount} on air</b> · listeners {behind}</div>
          {:else}
            <div class="sub"><b>Room alive</b> · {openCount} open to the room · listeners hear nothing of it</div>
          {/if}
        </div>
        {#if shown('delay', lk)}
        {@const dumpOff = off('dump')}
        <div class="delay" data-panel="desk.delay">
          {#if delay && delay.target_ms > 0}
            <svg class="arc" viewBox="0 0 48 48" aria-hidden="true">
              <circle class="bg" cx="24" cy="24" r="20" />
              <circle
                class="fg"
                class:warn={delay.rebuilding && delay.dumps > 0}
                cx="24"
                cy="24"
                r="20"
                stroke-dasharray="{(125.7 * Math.min(1, delay.delay_ms / delay.target_ms)).toFixed(1)} 125.7"
              />
              <text x="24" y="28" text-anchor="middle">{secs(delay.delay_ms)}</text>
            </svg>
          {/if}
          <div class="lbl">
            {#if !delay}
              <span class="state hollow hollow-standby" data-panel-title
                ><svg aria-hidden="true"><use href="#g-delay" /></svg>Delay</span
              >
              <span class="small">unknown · the desk is not answering</span>
            {:else if delay.target_ms === 0}
              <span class="state hollow hollow-standby" data-panel-title
                ><svg aria-hidden="true"><use href="#g-delay" /></svg>Delay off</span
              >
              <span class="small">nothing is delayed</span>
            {:else if delay.rebuilding && delay.dumps > 0}
              <StateChip chip={CHIP.dumped} data-panel-title />
              <span class="small"
                >rebuilding {secs(delay.delay_ms)} s{#if delay.dumped_at_ms !== null}&nbsp;· {hms(delay.dumped_at_ms)}{/if} · listeners
                now {secs(delay.delay_ms)} s behind</span
              >
            {:else if delay.rebuilding}
              <StateChip chip={CHIP.delay} word="Delay building" data-panel-title />
              <span class="small">{secs(delay.delay_ms)} of {secs(delay.target_ms)} s held · Dump throws it away</span>
            {:else}
              <!-- Full: the sentence for DELAY 3.2 S speaks of a rebuild, so this word carries none. -->
              <span class="state hollow hollow-accent" data-panel-title
                ><svg class="g-accent" aria-hidden="true"><use href="#g-delay" /></svg>Delay {secs(delay.target_ms)} s</span
              >
              <span class="small">full · listeners {secs(delay.delay_ms)} s behind</span>
            {/if}
          </div>
          <button
            type="button"
            class="btn big"
            class:danger-hollow={dumpable && !dumpOff}
            class:armed={dumpArmed}
            aria-disabled={dumpable && !dumpOff ? undefined : 'true'}
            disabled={prep}
            title={prepWhy ?? (dumpOff ? `Dump is off at ${STEP_WORD[step]}. It comes on at Callers.` : undefined)}
            onclick={() => !dumpOff && run(dumpArmed ? { type: 'dump' } : { type: 'arm', what: 'dump' })}
            >{dumpArmed ? 'Press again' : 'Dump'} <kbd>Del</kbd><kbd>Del</kbd></button
          >
          {#if dumpOff}<span class="offtag">{tag}</span>{/if}
        </div>
        {:else}
          <!-- The delay keeps running below Callers (ADR-0274); only its figure and Dump are out of sight. -->
          <span></span>
        {/if}
        <!-- End show belongs to every step, Solo included; the Screener button comes with Full. -->
        <div class="acts">
          <button type="button" class="btn quiet" disabled={prep} title={prepWhy} onclick={() => run({ type: 'end-show' })}>End show <kbd>⌃⇧E</kbd></button>
          {#if shown('screener-button', lk)}
            <button type="button" class="btn quiet" onclick={() => run({ type: 'tab', tab: 'screener' })}
              >Screener <kbd>⌃2</kbd></button
            >
          {/if}
        </div>
      </header>

      <div class="tiles">
        {#if ownLink}
          <!-- Before the DJ has joined (ADR-0274): one button, no name to type and no role to choose. -->
          <div class="empty join" data-testid="own-link">
            <b>You are not in the room yet</b>
            <span>Your voice reaches the desk from a browser tab on this computer. Open your microphone link, press Join the studio, and you are on air.</span>
            <Invite own />
          </div>
        {/if}
        {#each onStage as s (s.identity)}
          <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} handover={handover?.dj === s.identity ? handover : null} handoverOff={off('handover')} />
        {:else}
          {#if !ownLink}
            <!-- Says why the stage is empty and what to do (the operator's 1.0.2 run: "Nobody is
                 open" told them neither, and a number key on an empty strip did nothing). -->
            <div class="empty" data-testid="stage-empty">
              {#if !v?.desk.reachable}
                <b>No desk</b>
                <span>{v?.desk.error ?? 'Waiting for the control plane.'}</span>
              {:else if offStage.length}
                <b>Nobody is on air</b>
                <span>{offStage.length === 1 ? `${offStage[0].name} has joined and waits` : `${offStage.length} people have joined and wait`} under <b>Not on air</b>, below. Press their number, 1 to 5, to put them on air.</span>
              {:else}
                <b>Nobody is in the room yet</b>
                <span>People appear here when they join: send a guest an invitation, or join yourself with your own microphone link. Then press their number, 1 to 5, to put them on air.</span>
                {#if !off('invite')}<Invite />{/if}
                {#if account.me?.kind === 'station'}<span class="hint">This window is signed in with the station key, which has no microphone of its own: to join yourself, sign out (your name, top right) and sign in as yourself.</span>{/if}
              {/if}
            </div>
          {/if}
        {/each}
        {#if stepNoteShown(lk) && !ownLink && v?.desk.reachable}
          <!-- The guest's seat, empty at Solo: it says once why the desk is quiet, and offers the way up. -->
          <div class="empty seat" data-panel="desk.step-note">
            <b data-panel-title>Just you on this show</b>
            <span
              >{v.step.show ? `${v.step.show.name} runs at Solo.` : 'The station runs at Solo.'} Guests, callers and the screener’s seat are switched
              off, so nobody can arrive unannounced.</span
            >
            <button type="button" class="btn" aria-haspopup="dialog" onclick={() => (studio.stepMenu = true)}>Move up a step…</button>
          </div>
        {/if}
      </div>

      {#if shown('waiting', lk)}
        <div class="waiting" data-panel="desk.waiting">
          <div class="wh">
            <span data-panel-title>Not on air</span><span class="rule"
              >a source moves between the stage and this row only on its own On key · the stage keeps slot order · cue,
              talk and mute never move it · its keys never change · while playout is down the stage does not change: its
              sources say OPEN (ADR-0062)</span
            >
            {#if off('invite')}<span class="offtag">invitations {tag}</span>{:else}<Invite />{/if}
          </div>
          {#each offStage as s (s.identity)}
            <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} handover={handover?.dj === s.identity ? handover : null} handoverOff={off('handover')} />
          {/each}
          <Tile source={null} slot={6} playoutUp={up} playoutForMs={playout.for_ms} {mode} />
        </div>
      {/if}
    </main>

    <!-- right: the station -->
    <div class="col right" class:three={!deskCallersShown(lk) && !shown('picture', lk)}>
      <section class="panel audience" data-panel="desk.audience" aria-label="Listeners">
        <div class="ph"><h2>Listeners</h2><span class="spacer"></span></div>
        {#if audience.count === null}
          <div class="count num">—</div>
          <div class="split" role="status">{audience.detail}</div>
        {:else}
          <div class="count num">{audience.count.toLocaleString('en-GB')} <small>{audience.count === 1 ? 'listener' : 'listeners'} now</small></div>
          <div class="split">{audience.detail}</div>
          {#if audience.edges}<div class="split">{audience.edges}</div>{/if}
        {/if}
        {#if cdn}
          <p class="cdn-warning" role="status" data-warning="cdn">
            <svg class="g-warning" aria-hidden="true"><use href="#g-fault" /></svg>
            <span><b>{cdn.title}.</b> {cdn.message}</span>
          </p>
        {/if}
        <div class="st">
          {#if auto && v?.desk.reachable}
            <!-- ON AUTOMATION (ADR-0183): what listeners hear while automation holds the air. -->
            <span class="state hollow hollow-auto"><svg class="g-auto" aria-hidden="true"><use href="#g-auto" /></svg>On automation</span>
          {:else if up}
            <StateChip chip={CHIP.allGood} word="Hearing the desk" />
          {:else}
            <StateChip chip={CHIP.playoutDown} />
          {/if}
        </div>
      </section>

      {#if deskCallersShown(lk)}
        <!-- The queue on the desk, in the place the Picture panel has at Full: a DJ alone answers
             callers here, with the Screener tab hidden. Three rows at most, so the chat keeps room. -->
        <section class="panel callers" data-panel="desk.callers" aria-label="Callers">
          <div class="ph">
            <h2>Callers</h2>
            <span class="spacer"></span><span class="small">{queue.head}</span>
          </div>
          {#if queue.rows.length}
            <ol class="cq">
              {#each queue.rows as r (r.identity)}
                <li>
                  <span class="n">{r.n}</span>
                  <span class="who"><b>{r.name}</b><span>{r.topic || 'no topic given'}</span></span>
                  <span class="t num">{r.dropped ? 'link lost' : dur(now - r.since_ms)}</span>
                </li>
              {/each}
            </ol>
            {#if queue.more}<p class="more">and {queue.more} more waiting</p>{/if}
            <div class="acts">
              <button
                type="button"
                class="btn"
                aria-disabled={queue.holds || !queue.next ? 'true' : undefined}
                disabled={prep}
                title={prepWhy ?? (queue.next || queue.holds ? queue.title : 'Whoever is waiting has a dropped link, or no consent to record on file.')}
                onclick={() => run({ type: 'take-caller' })}
                >{queue.next ? `Take ${queue.next.name} to strip 4` : 'Take the next caller'} <kbd>P</kbd></button
              >
            </div>
          {:else}
            <p class="quiet-line">
              <b>Nobody is waiting.</b> Listeners call from the player page; each one appears here with their name and what they want to talk about.
            </p>
          {/if}
        </section>
      {/if}

      {#if shown('picture', lk)}
      {@const picOff = off('picture')}
      <section class="panel video" data-panel="desk.picture" aria-label="Picture">
        <div class="ph"><h2>Picture</h2>{#if picOff}<span class="spacer"></span><span class="offtag">{tag}</span>{/if}</div>
        {#if pic}
          <div class="pic" aria-label={pic.shot === 'slate' ? 'The slate is on air' : `${shotName} is on air`}>
            {#if pic.shot === 'slate'}
              <div class="slate"><b>{v?.station || 'WeaveSignal'}</b><span>{v?.show.title}</span><span class="pill">slate</span></div>
            {:else}
              <div class="slate"><b>{shotName}</b><span>on camera</span><span class="pill">{pic.mode.mode === 'auto' ? 'follows the audio' : 'held'}</span></div>
            {/if}
          </div>
          <div class="sw" role="group" aria-label="Picture source">
            <button type="button" class="btn" aria-pressed={pic.mode.mode !== 'slate'} aria-disabled={picOff || undefined} disabled={prep} title={prepWhy ?? (picOff ? `The picture is held on the slate at ${STEP_WORD[step]}. Switching it comes with Full.` : undefined)} onclick={() => !picOff && pic.mode.mode === 'slate' && run({ type: 'picture' })}
              >Live picture <kbd>F7</kbd></button
            >
            <button type="button" class="btn" aria-pressed={pic.mode.mode === 'slate'} aria-disabled={picOff || undefined} disabled={prep} title={prepWhy} onclick={() => !picOff && pic.mode.mode !== 'slate' && run({ type: 'picture' })}
              >Slate <kbd>F7</kbd></button
            >
          </div>
          <p class="small">{pic.cameras.length} {pic.cameras.length === 1 ? 'camera' : 'cameras'}{pic.hls ? '' : ' · encoders restarting'}</p>
          <Cameras
            cameras={pic.cameras}
            onair={pic.shot === 'person' ? (pic.identity ?? null) : null}
            label={(id) => {
              const s = v?.sources.find((x) => x.identity === id);
              return s ? firstName(s.name, id) : id;
            }}
          />
          <SlateStill on={!!pic.still} off={prep} />
        {:else}
          <div class="pic" aria-label="No programme picture">
            <div class="slate"><b>{v?.station || 'WeaveSignal'}</b><span>{v?.show.title}</span><span class="pill">{picOff ? 'the slate holds the picture' : 'video off'}</span></div>
          </div>
          <div class="sw" role="group" aria-label="Picture source">
            <button type="button" class="btn" aria-disabled="true" disabled={prep} title={prepWhy} onclick={() => !picOff && run({ type: 'picture' })}
              >Live picture <kbd>F7</kbd></button
            >
            <button type="button" class="btn" aria-disabled="true" disabled={prep} title={prepWhy} onclick={() => !picOff && run({ type: 'picture' })}
              >Slate <kbd>F7</kbd></button
            >
          </div>
        {/if}
      </section>
      {/if}

      <section class="panel" data-panel="desk.carts" aria-label="Carts">
        <div class="ph">
          <h2>Carts{#if glanceBank}&nbsp;· {glanceBank.name}{/if}</h2>
          <span class="spacer"></span><span class="small">bank {studio.bank}{#if glanceBank}, {bankOwner(glanceBank)}{/if} · <kbd>F1</kbd>–<kbd>F6</kbd></span>
        </div>
        <div class="carts" style="--cart-colour:{glanceBank ? `var(--cart-${glanceBank.colour_slot})` : 'var(--deemph)'}">
          {#each glance as { n, cart, now: c } (n)}
            {#if !cart}
              <button type="button" class="cart empty" aria-label="F{n}, empty slot" disabled={prep} title={prepWhy} onclick={() => run({ type: 'cart', n })}
                ><span class="label">Empty</span><span class="dur num">no cart</span><kbd>F{n}</kbd></button
              >
            {:else}
              <button
                type="button"
                class="cart"
                class:playing={!!c.sounding && c.state !== 'cued'}
                class:cued={c.state === 'cued'}
                class:ending={c.state === 'ending'}
                class:missing={c.state === 'missing'}
                aria-label="F{n}, {cart.label}{c.state === 'missing'
                  ? ', file missing, the key does nothing'
                  : c.state === 'cued'
                    ? ', cued, in your headphones'
                    : c.sounding
                    ? cart.mode === 'bed'
                      ? ', looping'
                      : `, playing, ${dur((c.leftMs ?? 0) + 999)} left`
                    : `, ${dur(cart.duration_ms + 999)}`}"
                aria-disabled={c.state === 'missing' || undefined}
                disabled={prep}
                title={prepWhy}
                onclick={() => run({ type: 'cart', n })}
                ><span class="label">{cart.label}</span><span class="dur num"
                  >{#if c.state === 'missing'}file missing{:else if c.sounding}<b
                      >{cart.mode === 'bed' ? 'loop' : `−${dur((c.leftMs ?? 0) + 999)}`}</b
                    >{:else}{dur(cart.duration_ms + 999)}{/if}</span
                >{#if c.sounding}<svg class="ring" viewBox="0 0 24 24" aria-hidden="true"
                    ><circle class="bg" cx="12" cy="12" r="9" /><circle
                      class="fg"
                      cx="12"
                      cy="12"
                      r="9"
                      stroke-dasharray={RING_C.toFixed(1)}
                      stroke-dashoffset={(RING_C * (1 - c.progress)).toFixed(1)}
                    /></svg
                  >{/if}<kbd>F{n}</kbd></button
              >
            {/if}
          {/each}
        </div>
      </section>

      <div class="slot">
        <!-- The chat to read, at every step: the room's last messages, and one line when something
             waits for a moderator (the chat's rules keep working out of sight, ADR-0274). -->
        <section class="panel chat" class:has-needs={!!needs} data-panel="desk.chat" aria-label="Chat">
          <div class="ph">
            <h2>Chat</h2>
            <span class="spacer"></span><span class="small">to read</span>
          </div>
          {#if needs}
            <div class="needs" role="status"><b>{needs}</b> {chatNeedsWhere(lk, v?.chat)}</div>
          {/if}
          <div class="msgs">
            {#if chatSays}<div class="msg link" data-chat="link">{chatSays}</div>{/if}
            {#each chatLines as m (m.id)}
              <div class="msg"><b>{m.author.name}</b> {m.body || (m.attachment ? `sent a ${m.attachment.kind}` : '')}</div>
            {/each}
            {#if priv}
              <div class="msg priv"><svg aria-hidden="true"><use href="#g-lock" /></svg><b>{priv.author.name}</b> {priv.body}</div>
            {/if}
          </div>
        </section>
        {#if toast}
          <div class="toast {toast.tone}" data-panel="desk.toast" role={toast.tone === 'good' || toast.tone === 'plain' ? 'status' : 'alert'}>
            <div class="stripe"></div>
            <div class="body">
              <svg aria-hidden="true"><use href="#{toast.glyph}" /></svg><span class="title" data-panel-title>{toast.title}</span>
              <span class="msg">{toast.msg}</span>
              {#if toast.acts.length}
                <span class="acts">
                  {#each toast.acts as a (a.label)}
                    <button type="button" class="btn" class:primary={a.primary} onclick={a.go}
                      >{a.label}
                      {#each a.keys as k, i (i)}<kbd>{k}</kbd>{/each}</button
                    >
                  {/each}
                </span>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>

  <footer class="statusbar" data-panel="desk.status" aria-label="Station status">
    <span class="item" data-panel-title
      ><svg class={v?.desk.reachable ? 'g-good' : 'g-critical'} aria-hidden="true"
        ><use href={v?.desk.reachable ? '#g-good' : '#g-fault'} /></svg
      >Room <b>LiveKit</b> · {peopleIn} in</span
    >
    {#if up}
      <span class="item"><svg class="g-good" aria-hidden="true"><use href="#g-good" /></svg>To playout <b>SRT connected</b></span>
    {:else}
      <span class="item"
        ><svg class="g-serious" aria-hidden="true"><use href="#g-playout" /></svg>To playout
        <b>no link{playout.for_ms !== null ? ` ${dur(playout.for_ms)}` : ''}</b> ·
        <button
          type="button"
          class="btn quiet"
          style="min-height:1.5rem;padding:0 var(--s-1)"
          disabled={prep}
          title={prepWhy}
          onclick={() => run({ type: 'restart-playout' })}>Restart <kbd>⌃R</kbd></button
        ></span
      >
    {/if}
    <span class="item"><svg class="g-muted" aria-hidden="true"><use href="#g-standby" /></svg>Listeners <b>icecast.server</b> · not watched yet</span>
    {#if degradedOnAir}
      <span class="item"
        ><svg class="g-warning" aria-hidden="true"><use href="#g-link-degraded" /></svg>Link degraded ·
        <b>{firstName(degradedOnAir.name, degradedOnAir.identity)}</b></span
      >
    {/if}
    <span class="sep"></span>
    {#if shown('talk-rule', lk)}
      {@const seatOff = off('talk-screener')}
      <span class="item"
        >Talk <b>A</b>–<b>H</b> held, not latched{#if shown('talk-screener', lk)}&nbsp;·
          <button
            type="button"
            class="btn quiet"
            class:lit-talk={!!screenerSeat && studio.talking.includes(screenerSeat)}
            style="min-height:1.5rem;padding:0 var(--s-1)"
            aria-label="Talk to the screener; held"
            aria-disabled={seatOff || undefined}
            disabled={prep}
            title={prepWhy ?? (seatOff ? `Talk to the screener is off at ${STEP_WORD[step]}. It comes with Full.` : undefined)}
            onpointerdown={() => !seatOff && run({ type: 'talk-seat', held: true })}
            onpointerup={() => run({ type: 'talk-seat', held: false })}
            >Talk to {seatOff ? 'the screener' : (screenerName ?? 'the screener')} · hold <kbd>⌃T</kbd></button
          >{#if seatOff}<span class="offtag">{tag}</span>{/if}{/if}</span
      >
    {/if}
  </footer>
</div>
