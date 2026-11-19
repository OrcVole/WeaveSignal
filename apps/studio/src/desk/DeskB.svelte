<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors -->
<script lang="ts">
  // Desk B, the focus (ADR-0061), wired to the desk through the control plane: time on the
  // left (clock, rundown, playout), what is on air in the middle (the lamp, the stage, the
  // sources not on air), the station on the right, and the station's health along the foot.
  // Panel names as on the design page (ADR-0070); a toast and the chat glance share the right
  // column's last cell, the toast above.
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
  //
  // The post's rule (ADR-0280; hosts-in-their-own-studios.md sections 3.4 to 3.6, 4.2 and 8) is
  // the station's, and the desk says what it is doing: the clock's post line counts the grace
  // with Hold, or says who keeps the air; the rundown has a row for the show whose post it is;
  // a late host is counted on air; a source whose link is lost keeps its tile on the stage; and
  // End show says where the air goes. A grace running is offered in the toast (On air on the
  // handover's key, Not now), and at Solo the next show's source has the guest's empty seat
  // (`desk.next`). The rules are in `lib/post.ts`.
  import { onMount } from 'svelte';
  import Tile from './Tile.svelte';
  import Hurry from './Hurry.svelte';
  import AirControl from './AirControl.svelte';
  import Cameras from './Cameras.svelte';
  import SlateStill from './SlateStill.svelte';
  import type { Notice } from '../lib/api.ts';
  import { CHIP, isOpen, listenersHear, masterLamp, sourceChip } from '../../../../packages/ui/states.ts';
  import StateChip from '../../../../packages/ui/StateChip.svelte';
  import { slotKeys } from '../../../../packages/ui/keymap.ts';
  import { look, studio, stationNow } from '../lib/studio.svelte.ts';
  import { deskToastNow, dismissGrace, dismissLost, holdPost, notYet, run, readyCaller } from '../lib/actions.ts';
  import { arrivals, countPct, countWords, endsToWords, goNow, graceToast, handNext, heldBy, hurry, keptPlace, keptSub, lateCount, lateToast, lostToast, mayHold, nextCard, ownStudiosLost, postLine, postRow, withKept, withoutNext } from '../lib/post.ts';
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
  import { atToday, delayLine, delayParts, delayWords, dur, firstName, hm, hms, listenerDelayMs, meterPct, signed } from '../lib/format.ts';
  import { arrival, arrivalWords, ownStudiosWords, tileLink } from '../lib/hosts.ts';
  import { sched } from '../lib/schedule.svelte.ts';
  import { setup } from '../lib/wizard.svelte.ts';
  import { bankAt, bankOwner, cartAt, cartNow } from '../../../../packages/ui/carts.ts';
  import { audienceWords, cdnWarning } from '../lib/audience.ts';
  import { HANDOVER_KEY, handoverButton, handoverStep } from '../lib/handover.ts';
  import Invite from './Invite.svelte';
  import { OWN_LINK, airHost, ownLinkBeside } from '../lib/invite.ts';
  import { preparing } from '../lib/prepare.svelte.ts';
  import { OFF_WHY } from '../lib/prepare.ts';
  import { edges } from '../schedule/rule/edges.ts';

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
  /** The clock's own line states the figure once (issue 17; the look's review, finding 3). */
  const behindLong = $derived(delayLine(v?.listener_delay));
  /** What the estimate is made of, in plain words, on the one line under it (ADR-0272, ADR-0289). */
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
  /** A host's own studio whose link is lost: its place is kept, and its tile stays on the stage. */
  const kept = $derived(keptPlace(v, studio.sourceSlots));
  /** At Solo the next show's source is one compact card in the guest's empty seat (`desk.next`;
   *  design section 4.2): Solo has no Not on air row, and that host is no part of this show's
   *  step. It has no On key: at Solo the air passes by End show or by the schedule. */
  const next = $derived(lk === 'solo' ? nextCard(v, tileLink) : null);
  const placed = $derived(withoutNext(withKept(stageOf(sources, lk), kept), next?.source ?? null));
  const onStage = $derived(placed.stage);
  const offStage = $derived(placed.waiting);
  /** The Solo stage offers the DJ their own microphone link until a presenter is in the room. */
  // Not in a preparing window: that link joins on air, which is not a preparing window's to do.
  const ownLink = $derived(!prep && !!v?.desk.reachable && needsOwnLink(sources, lk, !!v?.step?.may_change, !account.me || account.me.kind === 'staff'));
  /** A host in their own studio has the air (on air, or their place kept while their link is
   *  lost): the show is theirs, so the stage does not headline *You are not in the room yet* with
   *  its filled button. The offer is one quiet line beside Invite someone…, with words for this
   *  case; at Solo, which has no Not on air row, the same line takes the guest's seat. */
  const host = $derived(ownLink ? airHost(sources, kept) : null);
  const ownBeside = $derived(host ? ownLinkBeside(host, v?.mode) : null);
  /** The stage's headline offer: an empty studio waiting for its presenter. */
  const ownHead = $derived(ownLink && !host);
  /** The callers waiting on the desk, at the Callers step. */
  const queue = $derived(deskCallers(v));
  const openCount = $derived(sources.filter(isOpen).length);
  /** The handover button (ADR-0074): what ⇧⏎ does next, worked out once for the DJ's tile and
   *  the rundown's row, so the key and its buttons never differ. Null with nothing to press. */
  /** A host's own studio standing by after its post with nobody presenting: one press of ⇧⏎
   *  puts it on air, and the station counts it on by itself (the late count) unless told Not yet. */
  const go = $derived(goNow(v));
  /** The late count the station runs, whether or not the press is offered. */
  const counted = $derived(lateCount(v));
  /** The next show's source standing by while someone presents: the handover's key picks it first. */
  const nextUp = $derived(handNext(v));
  const handover = $derived(v && shown('handover', lk) ? handoverButton(handoverStep(sources, v.seats.presenter, studio.handoverTo, go, nextUp)) : null);
  /** Someone presenting is on air, as the station counts them. */
  const othersOnAir = $derived(!!v?.post?.holders.length);
  const lamp = $derived(
    masterLamp({ deskReachable: !!v?.desk.reachable, playoutConnected: playout.connected, anyOnAir: openCount > 0, mode }),
  );
  // A host's own studio is at the desk and not in the room (ADR-0280): it is counted on its own.
  const peopleIn = $derived(sources.filter((s) => s.present && s.role !== 'source').length + (v?.callers.filter((c) => c.present && c.slot === null).length ?? 0));
  /** The foot's Own studios item: what is connected, or nothing to say. */
  const ownStudios = $derived(ownStudiosLost(ownStudiosWords(sources), kept));

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
  const hardPost = $derived(rundown.find((r) => r.hard_post && r.ms > now) ?? null);
  /** The schedule's own line (ADR-0280): the next show's post, the grace, the late count, a held
   *  handover, or who keeps the air. Before the post, the nearer of it and the rundown's hard
   *  post has the line. */
  const line = $derived.by(() => {
    const l = postLine(v?.post, now);
    return l?.kind === 'post' && hardPost && hardPost.ms < l.at_ms ? null : l;
  });
  const post = $derived(line ? null : hardPost);
  const count = $derived(line?.kind === 'count' ? { is: line.why, ...countWords(line), left: Math.max(0, line.at_ms - now), pct: countPct(line, now) } : null);
  const canHold = $derived(mayHold(account.me));
  const holding = $derived(!!studio.holdAsked);
  /** The rundown's row for the show whose post it is: where it comes from and how it stands. */
  const showRow = $derived(postRow(v, now));
  const postFrom = $derived.by(() => {
    if (!post) return 0;
    const before = rundown.filter((r) => r.ms < post.ms && r.ms <= now).at(-1);
    return before?.ms ?? post.ms - 10 * 60_000;
  });
  const left = $derived(post ? post.ms - now : 0);
  const coming = $derived(rundown.filter((r, i) => r.ms > now || rundown[i + 1]?.ms > now).slice(0, showRow ? 4 : 5));
  /** Where the schedule's row sits among the rundown's: by its time. */
  const showRowAt = $derived(showRow ? coming.filter((r) => r.ms < showRow.at_ms).length : -1);
  const current = $derived(rundown.filter((r) => r.ms <= now).at(-1) ?? null);

  // ---- the toast slot: one toast, most urgent first ----
  type Toast = {
    /** `plain`: a note with no tone colour (a hidden part's key). */
    tone: 'good' | 'warning' | 'serious' | 'plain';
    glyph: string;
    title: string;
    /** A title whose end moves every second (a count): the words before it, the moving figure,
     *  drawn for the eye alone, and what a screen reader is told in its place, once. */
    count?: { lead: string; tick: string; said: string; left_ms: number };
    msg: string;
    /** `air`: the action changes what is on air, so a preparing window does not offer it.
     *  `off`: the action is drawn and cannot act yet; the reason is its tooltip. */
    acts: { label: string; keys: string[]; primary?: boolean; air?: boolean; off?: string; go: () => void }[];
  };
  const caller = $derived(readyCaller());
  const degradedOnAir = $derived(sources.find((s) => s.on_air && (!s.present || !s.linked)) ?? null);
  /** A host's own studio that has just arrived standing by, until it is cued, aired or dismissed. */
  const arrived = $derived(arrival(arrivals(v), studio.toldSources));
  const lastNotice: Notice | null = $derived(
    v?.notices
      .filter((n) => n.surface === 'desk' && !studio.dismissed.includes(n.id) && now - n.at_ms < 20_000 && n.kind !== 'caller.ready')
      .at(-1) ?? null,
  );
  /** A lost link that automation covers, and the late count: the two toasts the post's rule adds. */
  const lost = $derived(lostToast(v, studio.lostDismissed));
  const late = $derived(lateToast(v, now));
  /** The grace's offer: the handover's key brings the next host on now, where the handover is on. */
  const gracePress = $derived(!prep && handover?.kind === 'on' && (handover.dj === nextUp?.identity || handover.dj === v?.post?.source) && !off('handover'));
  const grace = $derived(graceToast(v, now, studio.graceDismissed, gracePress));
  /** Which toast with an answer is on screen: the same rule Esc answers by. */
  const kind = $derived(deskToastNow());
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
    if (kind === 'playout') {
      return {
        tone: 'serious',
        glyph: 'g-playout',
        title: 'Playout down · room alive',
        msg: `${playout.for_ms === null ? 'Liquidsoap has not connected to the desk.' : `SRT to Liquidsoap dropped ${dur(playout.for_ms)} ago.`} Listeners hear nothing of the room; the room keeps talking and its sources say OPEN.`,
        acts: [{ label: 'Restart playout', keys: ['⌃R', '⌃R'], primary: true, air: true, go: () => run({ type: 'restart-playout' }) }],
      };
    }
    if (kind === 'lost' && lost && kept) {
      // A link lost on air, once automation covers (design sections 3.6 and 8). Off air is the
      // strip's own On key, which lets the place go at once. Dismiss keeps the place.
      return {
        tone: 'serious',
        glyph: 'g-link-degraded',
        title: lost.title,
        msg: lost.message,
        acts: [
          { label: 'Off air', keys: [slotKeys(kept.slot).on], air: true, go: () => run({ type: 'on', slot: kept.slot }) },
          { label: 'Dismiss', keys: ['Esc'], go: dismissLost },
        ],
      };
    }
    if (kind === 'caller' && caller) {
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
    if (kind === 'degraded' && degradedOnAir) {
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
    if (kind === 'grace' && grace) {
      // A grace running (design sections 3.4 and 8). On air is the handover's key and brings the
      // next host on now, as the handover does; Not now only puts the offer away: the grace
      // runs on, and the schedule still hands over at its end.
      return {
        tone: 'warning',
        glyph: 'g-delay',
        title: grace.title,
        count: { lead: grace.lead, tick: grace.tick, said: grace.said, left_ms: grace.left_ms },
        msg: grace.message,
        acts: [
          ...(gracePress ? [{ label: 'On air', keys: [HANDOVER_KEY], primary: true, air: true, go: () => void run({ type: 'handover' }) }] : []),
          { label: 'Not now', keys: ['Esc'], go: dismissGrace },
        ],
      };
    }
    if (kind === 'late' && late) {
      // A late host's count (design sections 3.5 and 8). On air is the handover's key, so it is
      // offered where the handover is on (Full); Esc stops the count.
      return {
        tone: 'plain',
        glyph: 'g-delay',
        title: late.title,
        count: { lead: late.lead, tick: late.tick, said: late.said, left_ms: late.left_ms },
        msg: late.message,
        acts: [
          ...(handover?.kind === 'go' && !off('handover') ? [{ label: 'On air', keys: [HANDOVER_KEY], primary: true, air: true, go: () => void run({ type: 'handover' }) }] : []),
          // Not yet, as on the tile: it stops the count (the operator, 2026-11-16: Not now dismisses an
          // offer, Not yet stops a count).
          { label: 'Not yet', keys: ['Esc'], air: true, go: () => void run({ type: 'escape' }) },
        ],
      };
    }
    if (kind === 'arrived' && arrived) {
      // A host's own studio has connected in its slot (ADR-0280, design section 3.2): said once,
      // with Cue on the tile's own key. The least urgent toast with an answer.
      const k = slotKeys(arrived.slot);
      const w = arrivalWords(arrived, v?.step.next ?? null, [...sched.shows, ...setup.shows].find((x) => x.slug === arrived.source?.show)?.name ?? '', studio.zone || sched.zone);
      return {
        tone: 'plain',
        glyph: 'g-disc',
        title: w.title,
        msg: w.message,
        acts: [
          { label: 'Cue', keys: [k.cue], primary: true, air: true, go: () => run({ type: 'cue', slot: arrived.slot }) },
          { label: 'Dismiss', keys: ['Esc'], go: () => run({ type: 'escape' }) },
        ],
      };
    }
    if (lastNotice) return { tone: lastNotice.tone, glyph: 'g-good', title: lastNotice.title, msg: lastNotice.message, acts: [] };
    return null;
  });
  const toast: Toast | null = $derived.by(() => {
    const t = anyToast;
    // A preparing window reads the toast and is offered none of its on-air answers. An answer
    // that is this window's own (Dismiss, Not now) stays where no other answer changes the air.
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
        {#if count}
          <!-- The schedule's own count (design section 4.2): the grace, with Hold for an admin or a
               producer, or a late host's count. Amber in its last 30 seconds and red in its last 10,
               as every count on the desk is (14a), with the count's mark before a heavier figure. -->
          {@const h = hurry(count.left)}
          <div class="post" class:grace={count.is === 'grace'} data-post={count.is}>
            <span class="name">{count.name}</span><span class="left num" data-hurry={h?.level}><Hurry {h} />−{dur(count.left)}{#if h}<span class="sr-only"> {h.word}.</span>{/if}</span>
            <div class="bar"><i style:width="{count.pct}%"></i></div>
            <span class="why">{count.by}</span>
            {#if count.is === 'grace' && canHold}
              <button type="button" class="btn sm hold" disabled={holding} title="Stops the count for this handover. It is then by a press only." onclick={() => holdPost(true)}>Hold</button>
            {/if}
          </div>
        {:else if line?.kind === 'held'}
          <div class="post held" data-post="held">
            <span class="name">Handover held by {heldBy(line.by, account.me)}</span>
            <span class="why">a press only</span>
            {#if canHold}
              <button type="button" class="btn sm hold" disabled={holding} title="Lets the schedule count again, from where the post and the next host’s standing by put it." onclick={() => holdPost(false)}>Let it run</button>
            {/if}
          </div>
        {:else if line?.kind === 'words'}
          <div class="post says" data-post="words"><span class="name">{line.text}</span></div>
        {:else if line?.kind === 'post'}
          {@const left = Math.max(0, line.at_ms - now)}
          {@const h = hurry(left)}
          <div class="post" data-post="post">
            <span class="name">{line.name} · the post {hms(line.at_ms)}</span><span class="left num" data-hurry={h?.level}><Hurry {h} />−{dur(left)}{#if h}<span class="sr-only"> {h.word}.</span>{/if}</span>
            <div class="bar"><i style:width="{Math.min(100, Math.max(0, 100 - (left / (10 * 60_000)) * 100))}%"></i></div>
          </div>
        {:else if post}
          {@const h = hurry(left)}
          <div class="post">
            <span class="name">{post.title} · hard post {post.at.length === 5 ? `${post.at}:00` : post.at}</span><span
              class="left num"
              data-hurry={h?.level}><Hurry {h} />−{dur(left)}{#if h}<span class="sr-only"> {h.word}.</span>{/if}</span
            >
            <div class="bar"><i style:width="{Math.min(100, ((now - postFrom) / (post.ms - postFrom)) * 100)}%"></i></div>
          </div>
        {:else}
          <div class="post"><span class="name">No hard post ahead</span></div>
        {/if}
        <!-- The listener delay, once: the figure on this line, its parts on the line under it and
             nowhere else (no tooltip repeats them), in words a presenter reads on air. -->
        <div class="listen">
          <svg aria-hidden="true" style="width:12px;height:12px;vertical-align:-1px"><use href="#g-delay" /></svg>
          Listeners hear <b class="num">{hms(now - LISTENER_DELAY_MS)}</b> · {behindLong}
          {#if delayDetail}<span class="small" data-testid="delay-detail">{delayDetail}</span>{/if}
        </div>
      </section>

      <section class="panel" data-panel="desk.rundown" aria-label="Rundown">
        <div class="ph">
          <h2>Coming up</h2>
          <span class="spacer"></span><span class="small">{v?.show.title || 'No show set'}</span>
        </div>
        <div class="rundown">
          {#snippet scheduleRow()}
            {#if showRow}
              <!-- The show whose post it is (design section 4.2): where it comes from, how its source
                   stands, and the handover's button while there is a press to make for it. -->
              {@const hand = handover && handover.dj === showRow.identity ? handover : null}
              <!-- The chip sits under the words, as the handover's button does: beside them it would
                   cut the show's name and where it comes from at this column's width. -->
              <div class="row hand" class:now={showRow.now} data-testid="post-row">
                <span class="at num">{hm(showRow.at_ms)}</span>
                <span class="what"><b>{showRow.title}</b>{#if showRow.detail}<span>{showRow.detail}</span>{/if}</span>
                {#if hand || showRow.chip}
                  <span class="handrow">
                    {#if showRow.chip}<StateChip chip={showRow.chip} sm />{/if}
                    {#if hand}
                      <button type="button" class="btn" data-handover={hand.kind} aria-disabled={off('handover') || undefined} disabled={prep} title={prepWhy ?? (off('handover') ? `The handover is off at ${STEP_WORD[step]}. It comes on at Full.` : hand.then)} onclick={() => !off('handover') && run({ type: 'handover' })}
                        >{hand.label} <kbd>{HANDOVER_KEY}</kbd></button
                      >
                    {/if}
                  </span>
                {/if}
              </div>
            {/if}
          {/snippet}
          {#each coming as r, i (r.at + r.title)}
            {#if i === showRowAt}{@render scheduleRow()}{/if}
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
            {#if !showRow}
              <div class="row"><span class="at num">—</span><span class="what"><b>Nothing scheduled</b><span>nothing in <code>studio.rundown</code></span></span><span></span></div>
            {/if}
          {/each}
          {#if showRowAt >= coming.length}{@render scheduleRow()}{/if}
        </div>
        <div class="small"><span class="proposal">setting</span> rows from <code>studio.rundown</code>; {showRow ? 'the schedule adds the next show’s post' : 'the schedule does not fill this yet'}</div>
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
              >Restart playout <kbd>⌃R</kbd><kbd>⌃R</kbd></button
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
          {:else if auto && keptSub(kept)}
            <!-- Automation covers a lost link (ADR-0280): whose place is kept, and until when. -->
            <div class="sub" data-testid="kept-sub"><b>Automation holds the air</b> · {keptSub(kept)}</div>
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
              <!-- What the delay holds, not where listeners are: the stage's own line says that, once. -->
              <span class="small">rebuilding · {secs(delay.delay_ms)} s held{#if delay.dumped_at_ms !== null}&nbsp;· {hms(delay.dumped_at_ms)}{/if}</span>
            {:else if delay.rebuilding}
              <StateChip chip={CHIP.delay} word="Delay building" data-panel-title />
              <span class="small">{secs(delay.delay_ms)} of {secs(delay.target_ms)} s held · Dump throws it away</span>
            {:else}
              <!-- Full: the sentence for DELAY 3.2 S speaks of a rebuild, so this word carries none. -->
              <span class="state hollow hollow-accent" data-panel-title
                ><svg class="g-accent" aria-hidden="true"><use href="#g-delay" /></svg>Delay {secs(delay.target_ms)} s</span
              >
              <span class="small">full · {secs(delay.delay_ms)} s held</span>
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
        <!-- End show belongs to every step, Solo included; the Screener button comes with Full.
             End show is drawn as Hand to automation is, outlined at 13 px with its key cap (the
             look's review, question 7): of the two ways off the air, the proper one to end a show
             must not look the quieter. Screener stays quiet: it only changes the tab. -->
        <div class="acts">
          <button type="button" class="btn quiet outline" data-testid="end-show" disabled={prep} title={prepWhy ?? endsToWords(v) ?? undefined} onclick={() => run({ type: 'end-show' })}>End show <kbd>⌃⇧E</kbd></button>
          {#if shown('screener-button', lk)}
            <button type="button" class="btn quiet" onclick={() => run({ type: 'tab', tab: 'screener' })}
              >Screener <kbd>⌃2</kbd></button
            >
          {/if}
        </div>
      </header>

      <div class="tiles">
        {#if ownHead}
          <!-- Before the DJ has joined (ADR-0274): one button, no name to type and no role to choose. -->
          <div class="empty join" data-testid="own-link">
            <b>You are not in the room yet</b>
            <span>{auto ? OWN_LINK.stageAuto : OWN_LINK.stage}</span>
            <Invite own />
          </div>
        {/if}
        {#each onStage as s (s.identity)}
          <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} handover={handover?.dj === s.identity ? handover : null} handoverOff={off('handover')} kept={kept?.identity === s.identity ? kept : null} count={counted?.source.identity === s.identity ? { at_ms: counted.at_ms, key: counted.key } : null} {othersOnAir} />
        {:else}
          {#if !ownHead}
            <!-- Says why the stage is empty and what to do (the operator's 1.0.2 run: "Nobody is
                 open" told them neither, and a number key on an empty strip did nothing). -->
            <div class="empty" data-testid="stage-empty">
              {#if !v?.desk.reachable}
                <b>No desk</b>
                <span>{v?.desk.error ?? 'Waiting for the control plane.'}</span>
              {:else if offStage.length}
                <b>Nobody is on air</b>
                <span>{offStage.length === 1 ? (offStage[0].role === 'source' ? `${offStage[0].name} is standing by from their own studio` : `${offStage[0].name} has joined and waits`) : `${offStage.length} people have joined and wait`} under <b>Not on air</b>, below. Press their number, 1 to 5, to put them on air.</span>
              {:else}
                <b>Nobody is in the room yet</b>
                <span>People appear here when they join: send a guest an invitation, or join yourself with your own microphone link. Then press their number, 1 to 5, to put them on air.</span>
                {#if !off('invite')}<Invite />{/if}
                {#if account.me?.kind === 'station'}<span class="hint">This window is signed in with the station key, which has no microphone of its own: to join yourself, sign out (your name, top right) and sign in as yourself.</span>{/if}
              {/if}
            </div>
          {/if}
        {/each}
        {#if next}
          <!-- The next show's source, in the guest's empty seat at Solo (design section 4.2): its
               name and state, its meter, Cue on its strip's own key, and when the schedule
               brings it on. No On: at Solo the air passes by End show or by the schedule. -->
          {@const ns = next.source}
          {@const nk = slotKeys(ns.slot)}
          {@const nfirst = firstName(ns.name, ns.identity)}
          <section class="empty seat next" data-panel="desk.next" aria-label="Next: {next.title}">
            <span class="caps">Next</span>
            <b data-panel-title>{next.title}</b>
            <span class="row">
              <StateChip chip={sourceChip({ on_air: false, mute: ns.mute, cue: ns.cue, ready: false }, up, mode)} />
              <span class="small">{next.link}</span>
            </span>
            <div class="hmeter-wrap">
              <div class="hmeter" role="img" aria-label={ns.lufs === null ? 'No level' : `Short-term ${signed(ns.lufs)} LUFS, target −23`}>
                <div class="fill" style:width="{meterPct(ns.lufs)}%"></div>
                {#if ns.lufs !== null && ns.lufs > -22}<div class="over" style:left="{meterPct(-23)}%" style:width="{meterPct(ns.lufs) - meterPct(-23)}%"></div>{/if}
                {#if ns.lufs_peak !== null}<div class="peak" style:left="{meterPct(ns.lufs_peak)}%"></div>{/if}
                <div class="target" style:left="{meterPct(-23)}%"></div>
              </div>
            </div>
            {#if next.count}
              <!-- The late count (design section 3.5): Not yet stops it, a button and no key. -->
              {@const nh = hurry(next.count.at_ms - now)}
              <span class="when" data-testid="next-when" data-hurry={nh?.level}>On air in <Hurry h={nh} /><b class="num">{dur(next.count.at_ms - now)}</b> · {next.words}{#if nh}<span class="sr-only"> {nh.word}.</span>{/if}</span>
            {:else}
              <span class="when" data-testid="next-when">{next.words}</span>
            {/if}
            <span class="row acts">
              <button type="button" class="btn" class:lit-cue={ns.cue} aria-pressed={ns.cue} disabled={prep} title={prepWhy ?? `${nfirst} in your headphones; listeners do not hear them.`} onclick={() => run({ type: 'cue', slot: ns.slot })}
                ><svg aria-hidden="true"><use href="#g-cue" /></svg> Cue {nfirst} <kbd>{nk.cue}</kbd></button
              >
              {#if next.count}
                <button type="button" class="btn" disabled={prep} title={prepWhy ?? 'Stops the count. They stay standing by, and go on air by their own press.'} onclick={() => next.count && notYet(next.count.key)}>Not yet</button>
              {/if}
            </span>
          </section>
        {/if}
        {#if ownBeside && !shown('waiting', lk)}
          <!-- Solo has no Not on air row: while a host in their own studio has the air, the quiet
               offer takes the guest's seat. No headline and no filled button: the show is theirs. -->
          <div class="empty seat own" data-testid="own-link">
            <span id="own-beside">{ownBeside}</span>
            <Invite own beside="own-beside" />
          </div>
        {/if}
        {#if stepNoteShown(lk) && !next && !ownLink && v?.desk.reachable}
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
          <div class="wh" class:has-own={!!ownBeside}>
            <span data-panel-title>Not on air</span>{#if ownBeside}
              <!-- A host in their own studio has the air: joining is offered here, quietly, beside
                   Invite someone…, and the row's own note steps aside for it. -->
              <span class="own" id="own-beside" data-testid="own-link">{ownBeside}</span>
              <Invite own beside="own-beside" />
            {:else}<span class="rule"
                >a source moves between the stage and this row only on its own On key · the stage keeps slot order · cue,
                talk and mute never move it · its keys never change · while playout is down the stage does not change: its
                sources say OPEN (ADR-0062)</span
              >{/if}
            {#if off('invite')}<span class="offtag">invitations {tag}</span>{:else}<Invite />{/if}
          </div>
          {#each offStage as s (s.identity)}
            <Tile source={s} slot={s.slot} playoutUp={up} playoutForMs={playout.for_ms} {mode} handover={handover?.dj === s.identity ? handover : null} handoverOff={off('handover')} count={counted?.source.identity === s.identity ? { at_ms: counted.at_ms, key: counted.key } : null} />
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
             callers here, with the Screener tab hidden. Three rows at most. -->
        <section class="panel callers" data-panel="desk.callers" aria-label="Callers">
          <div class="ph">
            <h2>Callers</h2>
            <span class="spacer"></span><span class="small">{queue.head}</span>
          </div>
          {#if queue.rows.length}
            <!-- Where the column is short the rows scroll here and Take stays in sight; the list
                 is then a tab stop, so the keys reach it. -->
            <div class="fold" role="group" aria-label="Who is waiting" use:edges={{ keys: true }}>
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
            </div>
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
        <!-- Where the column is short (1,440 × 900 with video on) the panel gives way to the slot's
             floor and this part scrolls inside it: the picture and its switch first, the cameras
             and the slate picture under them. -->
        <div class="fold">
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
        </div>
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

      <!-- The slot a toast and the chat glance share (ADR-0070: `desk.toast`, `desk.chat`). The
           column keeps it a floor of height, so a toast always has its place and arrives without
           moving a panel above it. The toast is as tall as its words (fewer lines of its message
           where the slot is short, so a keyed answer is whole); the chat keeps what is left under
           it, and where that would not hold two lines it is left out whole, never drawn as a
           header with nothing to read (styles/desk-b.css). -->
      <div class="slot">
        {#if toast}
          <!-- A toast with a count keeps its own glyph, the clock, in the count's last seconds: it
               takes the level's colour, and the stripe stops at the warning colour. A count that
               runs out is going to plan, so the toast never takes a critical alert's look. -->
          {@const th = toast.count ? hurry(toast.count.left_ms) : null}
          <div class="toast {toast.tone}" data-panel="desk.toast" data-hurry={th?.level} role={toast.tone === 'good' || toast.tone === 'plain' ? 'status' : 'alert'}>
            <div class="stripe"></div>
            <div class="body">
              <svg aria-hidden="true"><use href="#{toast.glyph}" /></svg><span class="title" data-panel-title
                >{#if toast.count}{toast.count.lead} <span aria-hidden="true">{toast.count.tick}</span><span class="sr-only">{toast.count.said}</span>{#if th}<span class="sr-only"> {th.word}.</span>{/if}{:else}{toast.title}{/if}</span
              >
              <!-- In a short slot the message is held to fewer lines (styles/desk-b.css); its title has it whole. -->
              <span class="msg" title={toast.msg}>{toast.msg}</span>
              {#if toast.acts.length}
                <span class="acts">
                  {#each toast.acts as a (a.label)}
                    <button type="button" class="btn" class:primary={a.primary} aria-disabled={a.off ? 'true' : undefined} title={a.off} onclick={a.go}
                      >{a.label}
                      {#each a.keys as k, i (i)}<kbd>{k}</kbd>{/each}</button
                    >
                  {/each}
                </span>
              {/if}
            </div>
          </div>
        {/if}
        <div class="chat-cell">
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
        </div>
      </div>
    </div>
  </div>

  <footer class="statusbar" data-panel="desk.status" aria-label="Station status">
    <span class="item" data-panel-title
      ><svg class={v?.desk.reachable ? 'g-good' : 'g-critical'} aria-hidden="true"
        ><use href={v?.desk.reachable ? '#g-good' : '#g-fault'} /></svg
      >Room <b>LiveKit</b> · {peopleIn} in</span
    >
    {#if ownStudios}
      <!-- Hosts in their own studios (ADR-0280): what is connected. Not listening is Settings'
           to say; the studio's view does not carry it. -->
      <span class="item" data-testid="own-studios"
        >{#if kept && !kept.back}<svg class="g-serious" aria-hidden="true"><use href="#g-link-degraded" /></svg>{:else}<svg aria-hidden="true"><use href="#g-disc" /></svg>{/if}Own studios <b>{ownStudios}</b></span
      >
    {/if}
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
          onclick={() => run({ type: 'restart-playout' })}>Restart <kbd>⌃R</kbd><kbd>⌃R</kbd></button
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
