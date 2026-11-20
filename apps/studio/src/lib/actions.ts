// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What each key (and each button, which runs the same action) does: the keymap's actions
// turned into the control plane's commands. One place, so a button and its key never differ.

import { onStationChange } from './reset.ts';
import { tick } from 'svelte';
import * as api from './api.ts';
import { ARM_MS, cartKey, type Action } from '../../../../packages/ui/keymap.ts';
import { cartAt, cartNow, moveSlot } from '../../../../packages/ui/carts.ts';
import { look, say, setBank, stationNow, studio, takeView } from './studio.svelte.ts';
import { deskCallers, offNote, stationStep, type OffPart } from './steps.ts';
import { changesAir } from './prepare.ts';
import { followStation, nudge, preparing } from './prepare.svelte.ts';
import { closeOpen, keyView, openRota, sched, step as stepView, today, type Closed } from './schedule.svelte.ts';
import { openStations } from './stations.svelte.ts';
import { firstName } from './format.ts';
import { handoverStep } from './handover.ts';
import { account } from './account.svelte.ts';
import { isMine, leaveWords } from './invite.ts';
import { arrival, talkOffWhy } from './hosts.ts';
import { arrivals, deskToastKind, goNow, graceToast, handNext, lateCount, keptPlace, lateToast, letGoWords, lostId, lostToast, nextSource, whose, withoutLetGo, type DeskToastKind } from './post.ts';
import { endShowNote, modeNote, modeRefusal, needsConfirm, type AirAsk } from './air.ts';
import { chat, chatSay, entries, mod, needsEntry, noteMyHide, roomLine, studioMsg, type SorPicture, type SorTarget } from './chat.svelte.ts';
import { TIMEOUT_MIN, reasonLabel, sorReasonOf, sorReasonsFor, step, type Entry } from './chatModel.ts';
import type { Msg, Reason } from '../../../../packages/ui/chat.ts';
import type { SorThen } from '../../../../packages/ui/keymap.ts';

/** A cart's key, its button, the desk's glance and F1 to F6 all come here: fire the cart in
 *  that slot of the current bank, or fade it if it is sounding (keymap section 4). With `cue`
 *  (⇧ and the key) it goes into the presenter's headphones only, or comes out of them. */
export async function fireSlot(slot: number, cue = false): Promise<void> {
  if (preparing()) return nudge();
  const cart = cartAt(studio.catalogue, studio.bank, slot);
  if (!cart) {
    say(
      'good',
      'No cart here yet',
      `Slot ${cartKey(slot)} of this bank is empty. Picking a cart from the library is not built yet; a cart is uploaded to its slot, or comes with a pack.`,
    );
    return;
  }
  if (cartNow(cart, studio.view?.carts ?? null, stationNow()).state === 'missing') {
    say('serious', 'File missing', `“${cart.label}” has no file. Re-link it in the library. The key does nothing.`);
    return;
  }
  await send(api.carts({ op: cue ? 'cue' : 'toggle', cart: cart.id }));
}

function focusCart(slot: number) {
  studio.cartFocus = slot;
  document.querySelector<HTMLElement>(`[data-cart-slot="${slot}"]`)?.focus();
}

/** Who each held talk key opened, by key, so letting go closes that same talk even if the
 *  presenter's seat moved while it was held (the seat follows the air, ADR-0231). */
const heldTalk = new Map<string, { me: string; to: string }>();

// A change of station (ADR-0285). The frame lets every held key go first, and the releases
// are sent to the station being left; what is still here after that names people of that
// station, and must never be sent to the next.
onStationChange('actions', () => {
  heldTalk.clear();
  returnFocus = null;
  lettingGo = null;
});

/** The talk keys this window holds open, by key (a test's). */
export function heldTalkKeys(): string[] {
  return [...heldTalk.keys()];
}

async function talkHeld(key: string, held: boolean, open: { me: string; to: string } | null): Promise<void> {
  if (!held) {
    const h = heldTalk.get(key);
    heldTalk.delete(key);
    if (!h) return;
    await send(api.desk({ op: 'talk_to', identity: h.me, to: h.to, on: false }));
    talkingTo(h.to, false);
    return;
  }
  if (!open) return;
  heldTalk.set(key, open);
  await send(api.desk({ op: 'talk_to', identity: open.me, to: open.to, on: true }));
  talkingTo(open.to, true);
}

function sourceAt(slot: number) {
  return studio.view?.sources.find((s) => s.slot === slot) ?? null;
}

function name(identity: string): string {
  const v = studio.view;
  const p = v?.sources.find((s) => s.identity === identity) ?? v?.callers.find((c) => c.identity === identity);
  if (p) return firstName(p.name, identity);
  // A host's own studio whose link is lost has left the view; its kept place has its name.
  const kept = keptPlace(v, studio.sourceSlots);
  return kept?.identity === identity ? firstName(kept.name, identity) : identity;
}

async function send(p: Promise<api.View>): Promise<boolean> {
  try {
    takeView(await p);
    return true;
  } catch (e) {
    refused(e);
    return false;
  }
}

/** A refused command: the station's own sentence, as given. A 409 may be the station holding
 *  this window as preparing when the window thought it was live (ADR-0276), so the window asks
 *  who it is again and the frame follows. */
function refused(e: unknown): void {
  say('warning', 'Not done', e instanceof Error ? e.message : String(e));
  if (e instanceof api.ApiError && e.status === 409) void followStation();
}

/** The caller a drop means: on the screener the selected one; on the desk the caller on strip
 *  4, or else the caller on air; on any other tab the caller on air (keymap section 2). */
function dropTarget(): string | null {
  const v = studio.view;
  if (!v) return null;
  if (studio.tab === 'screener') return studio.selected;
  const s4 = sourceAt(4);
  if (studio.tab === 'desk' && s4?.role === 'caller') return s4.identity;
  return v.callers.find((c) => c.on_air)?.identity ?? null;
}

function talkingTo(to: string, held: boolean) {
  studio.talking = held ? [...new Set([...studio.talking, to])] : studio.talking.filter((t) => t !== to);
}

/** The desk's toast that Esc answers: a caller sent by the screener and waiting for the
 *  presenter. Not at the Callers step, where the DJ took the call themselves and there is no
 *  screener to answer. That is the station's step, not this window's look: with everything
 *  shown at Callers the call is still the DJ's own. */
export function readyCaller() {
  const v = studio.view;
  if (stationStep(v) === 'callers') return null;
  return v?.sources.find((s) => s.role === 'caller' && s.caller?.sent && !s.caller.taken && !s.on_air && !s.cue) ?? null;
}

/** Which of the desk's toasts with an answer is on screen now, the most urgent first: the
 *  toast slot draws it and Esc answers it, by this one rule. */
export function deskToastNow(): DeskToastKind | null {
  const v = studio.view;
  if (!v) return null;
  return deskToastKind({
    playoutDown: v.desk.reachable && !v.desk.playout.connected,
    lost: !!lostToast(v, studio.lostDismissed),
    caller: !!readyCaller(),
    degraded: v.sources.some((s) => s.on_air && (!s.present || !s.linked)),
    grace: !!graceToast(v, stationNow(), studio.graceDismissed),
    late: !!lateToast(v, stationNow()),
    arrived: !!arrival(arrivals(v), studio.toldSources),
  });
}

// ---- the post's rule, from the desk (ADR-0280): Hold, Go on air now, Not yet ----

/** Hold on the clock's post line, or Let it run: for an admin or a producer, a button and no
 *  key. The station answers without a view and the view follows within a second, so the button
 *  waits for it: a second press would undo the first. */
export async function holdPost(on: boolean): Promise<void> {
  if (studio.holdAsked) return;
  studio.holdAsked = { was: studio.view?.post?.held_by ?? null, at: Date.now() };
  try {
    await (on ? api.holdPost() : api.releasePost());
  } catch (e) {
    studio.holdAsked = null;
    refused(e);
  }
}

/** Go on air now: the source standing by takes the air from automation at once. The handover's
 *  key and its buttons come here. */
export async function goOnAir(key: string, who: string): Promise<void> {
  if (preparing()) return nudge();
  try {
    await api.sourceGo(key);
    say('good', `${who} is on air`, 'By a press: the air is taken back from automation.');
  } catch (e) {
    refused(e);
  }
}

/** Not yet: the late count stops, and the source goes on air by a press. The count leaves the
 *  desk at once; the station's next view confirms it. */
export async function notYet(key: string): Promise<void> {
  if (preparing()) return nudge();
  const who = lateCount(studio.view)?.source ?? goNow(studio.view)?.source ?? null;
  try {
    await api.sourceNotYet(key);
    if (studio.view?.post) studio.view = { ...studio.view, post: { ...studio.view.post, late_ends_ms: null, on_air_at_ms: null } };
    say('good', 'The count is stopped', `${who ? firstName(who.name, who.identity) : 'The host'} stays standing by. To put them on air, press ⇧⏎ at Full, or take the air back and press their On key.`, 8000);
  } catch (e) {
    refused(e);
  }
}

/** Dismiss on the lost-link toast: the place stays kept, and the tile goes on saying so. */
export function dismissLost(): void {
  const t = lostToast(studio.view, studio.lostDismissed);
  if (t) studio.lostDismissed = [...studio.lostDismissed, t.id];
}

/** Not now on the grace's toast: the offer goes and nothing is sent. The grace runs on, the
 *  clock goes on counting it, and the schedule still hands over at its end. */
export function dismissGrace(): void {
  const t = graceToast(studio.view, stationNow(), studio.graceDismissed);
  if (t) studio.graceDismissed = [...studio.graceDismissed, t.id];
}

/** The key whose Off air is on its way to the station: a second press waits for the first. */
let lettingGo: string | null = null;

/** Off air on a kept place (design section 3.6): the toast's button, and the kept tile's own On
 *  key. The station lets the place go at once, and says whether automation took the air with
 *  it. Its answer has no view, so the place leaves this window's at once: the tile goes, and a
 *  second press is not sent to be refused. */
export async function offAirKept(key: string): Promise<void> {
  if (preparing()) return nudge();
  if (lettingGo === key) return;
  const before = studio.view;
  const lost = before?.post?.link_lost;
  lettingGo = key;
  try {
    const r = await api.sourceOffAir(key);
    // The station's view follows within a second. Until it does, this window reads every view
    // without the place (`withoutLetGo`): the tile and the rundown's row go at once, and stay gone.
    const v = studio.view;
    const now = v?.post?.link_lost;
    if (v && now?.key_id === key) {
      studio.letGo = [...studio.letGo, lostId(now)];
      studio.view = withoutLetGo(v, studio.letGo);
    }
    const w = letGoWords(lost?.key_id === key ? lost.name : 'The host', !!r.covered, before);
    say('good', w.title, w.message, 10000);
  } catch (e) {
    refused(e);
  } finally {
    lettingGo = null;
  }
}

/** The kept place on a strip, when the strip's own source has left the desk. */
function keptAt(slot: number) {
  const k = keptPlace(studio.view, studio.sourceSlots);
  return k && !k.back && k.slot === slot ? k : null;
}

/** How long a hidden part's note stays in the toast slot. */
const OFF_NOTE_MS = 12_000;

/** A key whose part the show's step keeps off does nothing; the first press in a session says
 *  so, once a part, in the desk's toast slot (design section 2). Away from the desk there is
 *  no slot to say it in, so that press is not counted as the first. */
export function tellOff(part: OffPart): void {
  if (studio.tab !== 'desk' || studio.settings || studio.offTold.includes(part)) return;
  const n = offNote(part, stationStep(studio.view));
  if (!n) return;
  studio.offTold = [...studio.offTold, part];
  studio.offNote = { ...n, until: Date.now() + OFF_NOTE_MS };
}

/** After Esc closed what the Schedule tab's editor column held: a focus that was inside the
 *  column, which is gone, goes back to where the person came from, the block in the grid or the
 *  show's row in the list. A focus anywhere else (a block, the toolbar) stays where it is. */
function focusAfterClose(closed: Extract<Closed, { did: 'closed' }>, from: Element | null): void {
  if (!from?.closest?.('.schedule aside.editor')) return;
  // The grid draws its blocks again once the selection has gone, and the list is drawn then too.
  void tick().then(() =>
    requestAnimationFrame(() => {
      const block = closed.block ? [...document.querySelectorAll<HTMLElement>('.schedule .ec-event[data-key]')].find((el) => el.dataset.key === closed.block || el.dataset.key?.startsWith(`${closed.block}@`)) : null;
      const list = document.querySelector<HTMLElement>('.schedule [data-panel="calendar.shows"]');
      const row = closed.show ? list?.querySelector<HTMLElement>(`[data-show="${closed.show}"]`) : null;
      (block ?? row ?? list?.querySelector<HTMLElement>('[data-show][tabindex="0"]'))?.focus({ preventScroll: !!block });
    }),
  );
}

export async function run(a: Action): Promise<void> {
  const v = studio.view;
  const now = Date.now();
  // A preparing window cannot change what is on air (ADR-0276): the action does nothing, for a
  // key and its button alike, and the band under the frame answers.
  const prep = preparing();
  if (prep && changesAir(a)) {
    studio.armed = null;
    return nudge();
  }
  switch (a.type) {
    case 'tab':
      // ⌃4 is the Room: the chat is moderated on the Screener tab, so the key opens that tab and
      // puts the focus in the Room lane. A Chat tab of its own may take the key back later.
      if (a.tab === 'chat') {
        studio.tab = 'screener';
        if (typeof document !== 'undefined') {
          document.documentElement.dataset.surface = 'screener';
          await focusRoom();
        }
      } else {
        studio.tab = a.tab;
        if (typeof document !== 'undefined') document.documentElement.dataset.surface = a.tab;
      }
      return;
    case 'arm':
      studio.armed = { what: a.what, at: now, identity: a.identity };
      // ⌃R has no button on a healthy station to show the armed state, so the first press says
      // what the second does, for as long as it stays armed.
      if (a.what === 'restart-playout') {
        say('good', 'Restart playout?', 'Press ⌃R again within 3 seconds to restart it. Listeners hear the fallback for a few seconds. Esc cancels.', ARM_MS);
      }
      if (a.what === 'ban') {
        const t = messageTarget();
        chat.banAt = t && 'line' in t ? 'room' : 'needs';
      }
      return;
    case 'disarm':
      studio.armed = null;
      return;
    case 'leave-field': {
      const el = document.activeElement as HTMLElement | null;
      el?.blur();
      el?.closest('[data-panel]')?.querySelector<HTMLElement>('[data-panel-title], h2')?.focus();
      return;
    }
    case 'escape': {
      if (studio.offNote) {
        studio.offNote = null;
        return;
      }
      if (studio.note) {
        studio.note = null;
        return;
      }
      if (chat.toast && chat.toast.until > Date.now()) {
        chat.toast = null;
        return;
      }
      // A host's lost link, a grace or a late count (ADR-0280), when it is the toast on screen:
      // Dismiss keeps the place and Not now only puts the offer away, both this window's own;
      // Not yet stops the station's count, which is not a preparing window's to do.
      if (studio.tab === 'desk') {
        const kind = deskToastNow();
        if (kind === 'lost') return dismissLost();
        if (kind === 'grace') return dismissGrace();
        const late = kind === 'late' && !prep ? lateToast(v, stationNow()) : null;
        if (late) return notYet(late.key);
      }
      // Esc's two sends (decline the caller who is ready, clear the cues) are not a preparing
      // window's to make; it still dismisses what is this window's own.
      if (studio.tab === 'desk' && !prep) {
        const c = readyCaller();
        if (c) {
          await send(api.screener({ op: 'decline', identity: c.identity, reason: 'not now' }));
          return;
        }
      }
      // On the Carts tab Esc takes every cued cart out of the headphones (cart-wall.md §4).
      if (studio.tab === 'carts' && !prep && v?.carts.sounding.some((s) => s.cue && !s.stopping)) {
        await send(api.carts({ op: 'clear_cue' }));
        return;
      }
      // A host's own studio that has just arrived standing by (ADR-0280): told, and dismissed.
      if (studio.tab === 'desk') {
        const came = arrival(arrivals(v), studio.toldSources);
        if (came) {
          studio.toldSources = [...studio.toldSources, came.identity];
          return;
        }
      }
      // On the Schedule tab Esc is ‹ Shows: it closes what the editor column holds and clears
      // the selection, unless an editor holds changes that are not saved (the tab then says so).
      // The selection is this window's own, so a preparing window's Esc clears it too.
      if (studio.tab === 'schedule') {
        const from = typeof document === 'undefined' ? null : document.activeElement;
        const closed = closeOpen(stationNow());
        if (closed.did === 'closed') focusAfterClose(closed, from);
        if (closed.did !== 'nothing') return;
      }
      const top = v?.notices.filter((n) => n.surface === studio.tab).at(-1);
      if (top) studio.dismissed = [...studio.dismissed, top.id];
      return;
    }
    case 'drop': {
      studio.armed = null;
      const id = dropTarget();
      if (!id) {
        say('good', 'No caller to drop', 'Nobody is on the line here.');
        return;
      }
      await send(api.screener({ op: 'drop', identity: id }));
      if (studio.selected === id) studio.selected = null;
      return;
    }
    case 'end-pass': {
      // The studio ends anyone's pass (ADR-0119): the name is taken before the send, because
      // the view that comes back no longer has them.
      studio.armed = null;
      const who = name(a.identity);
      const mine = isMine(a.identity, account.me);
      if (await send(api.endPass({ identity: a.identity }))) {
        if (mine) {
          const w = leaveWords(studio.view, account.me?.kind === 'staff', look());
          say('good', w.title, w.message);
        }
        else say('good', 'Pass ended', `${who} is out of the room and their link no longer works. Send a new invitation if they are to come back.`);
      }
      return;
    }
    case 'disconnect': {
      // A host's own studio (ADR-0280): the name is taken before the send, because the tile
      // leaves with the connection. The view follows by itself, as the desk reports it gone.
      studio.armed = null;
      const who = name(a.identity);
      try {
        await api.disconnectSource(a.key);
        say('good', `${who} is disconnected`, 'Their key is turned away for the rest of today’s show, so software that reconnects by itself stays out. Let them back in, or end the key, in Settings › Own studios.');
      } catch (e) {
        refused(e);
      }
      return;
    }
    case 'picture': {
      // F7: the slate held, or back to the live picture (the vision mixer following the
      // audio). The audio controls never move (desk design).
      const p = v?.desk.picture;
      if (!p) return void say('good', 'Video is off', 'This station runs without video (ADR-0009); there is no picture to switch.');
      await send(api.desk({ op: 'picture', mode: p.mode.mode === 'slate' ? 'auto' : 'slate' }));
      return;
    }
    case 'end-show':
      // The dialog is on the desk: bring the desk up so it can be answered (keymap rule 5).
      studio.armed = null;
      if (!v) return;
      studio.tab = 'desk';
      if (typeof document !== 'undefined') document.documentElement.dataset.surface = 'desk';
      studio.airAsk = 'end';
      return;
    case 'dump': {
      // The report lives in the delay line, not a toast (design finding 24); only a dump that
      // cannot happen says why.
      studio.armed = null;
      const d = v?.desk.delay;
      if (!d) return void say('warning', 'Not dumped', 'The desk is not answering, so its delay cannot be dumped from here.');
      if (d.target_ms === 0) return void say('good', 'No delay to dump', 'The desk runs with the profanity delay off; listeners hear the room as it happens.');
      if (d.delay_ms === 0)
        return void say('warning', 'Nothing held', `The delay holds nothing yet (it is rebuilding to ${(d.target_ms / 1000).toFixed(0)} s); Dump works as soon as it holds some audio.`);
      await send(api.desk({ op: 'dump' }));
      return;
    }
    case 'restart-playout': {
      // ⌃R twice, or the Restart button where playout is down. The station asks playout to shut
      // down and its supervisor starts it again; the view then shows the link coming back.
      studio.armed = null;
      try {
        const r = await api.restartPlayout();
        say('good', 'Playout is restarting', r.words);
      } catch (e) {
        refused(e);
      }
      return;
    }
    case 'influx':
      studio.armed = null;
      if (v) await send(api.chat({ op: 'influx', on: !v.chat.influx }));
      return;
    case 'handover': {
      if (!v) return;
      const step = handoverStep(v.sources, v.seats.presenter, studio.handoverTo, goNow(v), handNext(v));
      // A host's own studio standing by with nobody presenting (ADR-0280): one press.
      if (step.kind === 'go') {
        studio.handoverTo = null;
        return goOnAir(step.key, firstName(step.dj.name, step.dj.identity));
      }
      if (step.kind === 'none') {
        studio.handoverTo = null;
        return void say('good', 'No DJ to hand over to', 'Invite the next DJ with Invite someone…, beside Not on air, keeping the role DJ. They wait there off air; ⇧⏎ then brings them on air with you.');
      }
      const dj = firstName(step.dj.name, step.dj.identity);
      // A host's own studio has no microphone in the room: the seat and the talk keys never pass to it.
      const own = step.dj.role === 'source';
      if (step.kind === 'done') {
        studio.handoverTo = null;
        return void say('good', `${dj} has the air`, own ? `The handover is done: ${dj} holds the air from their own studio.` : `The handover is done: ${dj} holds the air and the desk's talk keys.`);
      }
      if (step.kind === 'on') {
        if (await send(api.desk({ op: 'on_air', identity: step.dj.identity, on: true }))) {
          studio.handoverTo = step.dj.identity;
          if (own) say('good', `${dj} is on air too`, `The overlap: both sound together, each at their own level. Press ⇧⏎ again to hand ${dj} the air.`, 15000);
          else say('good', `${dj} is on air with you`, `The overlap: say hello. Press ⇧⏎ again to take yourself off and hand ${dj} the air.`, 15000);
        }
        return;
      }
      for (const s of step.off) if (!(await send(api.desk({ op: 'on_air', identity: s.identity, on: false })))) return;
      studio.handoverTo = null;
      const off = step.off.map((s) => firstName(s.name, s.identity)).join(' and ');
      if (own) say('good', `Handed over to ${dj}`, `${off} off air; ${dj} holds the air from their own studio.`, 12000);
      else say('good', `Handed over to ${dj}`, `${off} off air; ${dj} holds the air. This window's talk keys now speak as ${dj}: close it, or hand it to them, and close your invitation page when you leave (ADR-0239).`, 12000);
      return;
    }
    case 'talk-seat': {
      if (!v) return;
      if (studio.tab === 'screener') {
        const me = v.seats.screener;
        if (!me) return void say('warning', 'No screener in the room', 'Join the room as the screener to talk to the presenter.');
        await send(api.desk({ op: 'talk', identity: me, on: a.held }));
        talkingTo('presenter', a.held);
      } else {
        const me = v.seats.presenter;
        const to = v.seats.screener;
        if (a.held && (!me || !to)) return void say('good', 'Nobody to talk to', 'There is no screener in the room.');
        await talkHeld('seat', a.held, me && to ? { me, to } : null);
      }
      return;
    }
    case 'on':
    case 'cue':
    case 'mute': {
      if (a.slot === 6) return void say('good', 'Playout is not a desk source yet', 'Liquidsoap mixes the desk in; a switch for playout at the desk is not built yet.');
      const s = sourceAt(a.slot);
      // A strip whose source has lost its link is not empty: its place is kept (ADR-0280).
      const kept = s ? null : keptAt(a.slot);
      if (kept) {
        const who = firstName(kept.name, kept.identity);
        if (a.type !== 'on') return void say('good', `Nothing to ${a.type}`, `${whose(who)} link is lost: nothing arrives from their software.`);
        // Their own On key is Off air: it lets the place go at once (design section 3.6).
        return offAirKept(kept.key);
      }
      // An empty strip says so, rather than nothing happening (the operator's 1.0.2 run).
      if (!s) return void say('good', `Nobody is on strip ${a.slot}`, 'A strip fills when someone joins the room: invite them with Invite someone… on the desk, or join with your own microphone link. Then this key puts them on air.');
      // At Solo the next show's source has the guest's seat and no On key (`desk.next`; design
      // section 4.2): its strip's number says where the air passes instead.
      if (a.type === 'on' && !s.on_air && look() === 'solo' && nextSource(v)?.identity === s.identity) {
        const who = firstName(s.name, s.identity);
        return void say('good', `${who} has no On key at Solo`, `The air passes to ${who} when you end the show, or by the schedule.`, 8000);
      }
      const field = a.type === 'on' ? 'on_air' : a.type;
      await send(api.desk({ op: field, identity: s.identity, on: !s[field] }));
      return;
    }
    case 'take-caller': {
      // `desk.callers` at the Callers step (ADR-0275): the caller who has waited longest goes
      // to the callers' strip, off air, by the same send the Screener tab makes. The strip
      // holds one caller, and the station refuses a second send at Callers (`Studio::send`).
      const q = deskCallers(v);
      if (q.holds) return void say('good', `Strip ${q.holds.slot} is taken`, q.title);
      if (!q.next) {
        return void (q.rows.length
          ? say('good', 'Nobody can be taken yet', 'Whoever is waiting has a dropped link, or no consent to record on file.')
          : say('good', 'Nobody is waiting', 'Listeners call from the player page; each one appears in Callers with their name and what they want to talk about.'));
      }
      await send(api.screener({ op: 'send', identity: q.next.identity }));
      return;
    }
    case 'talk': {
      const s = sourceAt(a.slot);
      const me = v?.seats.presenter;
      // A host's own studio has no ears in the room: Talk reaches their monitor link, while it
      // is open (ADR-0280; design section 4.1). It speaks as the presenter in the room, as ever.
      // Letting go always goes through: a talk held open is closed whatever has changed since.
      if (s?.role === 'source' && a.held) {
        const who = firstName(s.name, s.identity);
        if (!s.source?.monitor) return void say('good', `${who} would not hear you`, talkOffWhy(who));
        if (!me) return void say('good', 'Nobody to talk as', `Talk is the voice of the presenter in the room, and nobody holds that seat. Join with your own microphone link, then hold this key to talk to ${who}.`);
      }
      await talkHeld(`talk:${a.slot}`, a.held, s && me && s.identity !== me ? { me, to: s.identity } : null);
      return;
    }
    case 'queue-move':
      if (moveInPanel(a.delta)) return;
    // falls through: the caller queue
    case 'queue-jump': {
      const list = v?.callers ?? [];
      if (!list.length) return;
      const at = list.findIndex((c) => c.identity === studio.selected);
      const i = a.type === 'queue-jump' ? a.n - 1 : Math.max(0, Math.min(list.length - 1, (at < 0 ? -1 : at) + a.delta));
      if (list[i]) studio.selected = list[i].identity;
      return;
    }
    case 'screen-talk': {
      // Letting go closes exactly the talk this key opened, whatever has moved since (the
      // selection, a screener joining): a talk_to left open keeps the talker off the programme.
      if (!a.held) return talkHeld('screen-talk', false, null);
      const id = studio.selected;
      // Talk on this tab is the screener's seat's, or the presenter's when nobody screens (issue
      // 25): while it is held the desk carries the talker's voice to the caller alone and the
      // caller's answer to the talker alone; the caller stays off air and out of the cue.
      const me = v?.seats.screener ?? v?.seats.presenter;
      if (!id || !me)
        return void (
          a.held &&
          say(
            'good',
            'Nobody to talk to',
            !id ? 'Select a caller first (↑ ↓ or 1 to 9).' : 'Join the room as the screener or the presenter.',
          )
        );
      const c = v?.callers.find((x) => x.identity === id);
      if (c?.state === 'waiting') await send(api.screener({ op: 'screen', identity: id }));
      await talkHeld('screen-talk', true, { me, to: id });
      return;
    }
    case 'send':
    case 'back':
      if (studio.selected) await send(api.screener({ op: a.type, identity: studio.selected }));
      return;
    case 'focus-name':
      document.getElementById('f-name')?.focus();
      return;
    case 'release-place': {
      const id =
        v?.callers.find((c) => c.identity === studio.selected && c.dropped_ms !== null)?.identity ??
        v?.callers.find((c) => c.dropped_ms !== null)?.identity;
      if (id) await send(api.screener({ op: 'release', identity: id }));
      return;
    }
    case 'mode':
      if (v) await send(api.chat({ op: 'mode', mode: a.mode, on: !v.chat[a.mode] }));
      return;
    case 'cart':
    case 'cart-key':
      await fireSlot(a.type === 'cart' ? a.n : a.slot);
      return;
    case 'cart-cue':
      await fireSlot(a.slot, true);
      return;
    case 'cart-stop-last':
      // Space stops the last cart on air; a cue is Esc's.
      if (v?.carts.sounding.some((s) => !s.stopping && !s.cue)) await send(api.carts({ op: 'stop_last' }));
      return;
    case 'fade-all':
      studio.armed = null;
      if (v?.carts.sounding.length) await send(api.carts({ op: 'fade_all' }));
      return;
    case 'bank':
      setBank(a.n);
      return;
    case 'cart-move':
      focusCart(moveSlot(studio.cartFocus ?? 1, a.dx, a.dy));
      return;
    case 'cart-enter':
      if (studio.cartFocus) await fireSlot(studio.cartFocus);
      return;
    // The rota is of the coming hours whatever week the calendar is on: the week's keys rest.
    case 'week':
      if (!sched.rota) stepView(a.delta);
      return;
    case 'today':
      if (!sched.rota) today(stationNow());
      return;
    case 'view':
      if (a.view === 'rota') openRota();
      else keyView(a.view, stationNow());
      return;
    case 'stations':
      openStations();
      return;
    case 'mod-hide':
    case 'mod-timeout':
    case 'ban':
    case 'mod-leave':
    case 'mod-release':
    case 'release-all':
    case 'hide-all':
    case 'acknowledge':
    case 'escalate':
    case 'feature':
    case 'pin':
    case 'invite':
    case 'reply':
    case 'pass':
    case 'newest':
    case 'post':
    case 'avatar-remove':
    case 'attachment-remove':
    case 'sor-then':
    case 'sor-reason':
    case 'sor-send':
    case 'sor-cancel':
      await chatAction(a);
      return;
  }
}

/** ⌃4: the focus goes to the Room's newest (or selected) line once the Screener tab is drawn. */
async function focusRoom(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await tick();
    const room = document.querySelector<HTMLElement>('[data-panel="screener.room"]');
    if (!room) continue;
    const line = room.querySelector<HTMLElement>('[data-chat="room-line"][tabindex="0"]');
    const head = room.querySelector<HTMLElement>('h2');
    const target = line ?? head;
    if (head && !line && !head.hasAttribute('tabindex')) head.setAttribute('tabindex', '-1');
    target?.focus();
    return;
  }
}

export { name as personName };

// ---- ending the show and the station's mode (ADR-0272) ----

/** The button beside the lamp: ask first when someone would be cut off or put on air, else just do it. */
export async function pressMode(ask: Exclude<AirAsk, 'end'>): Promise<void> {
  const v = studio.view;
  if (!v) return;
  if (preparing()) return nudge();
  const why = modeRefusal(v);
  if (why) return void say('good', 'No automation here', why);
  if (needsConfirm(ask, v)) {
    studio.airAsk = ask;
    return;
  }
  await confirmAir(ask);
}

/** What the air dialog's confirm does: the same call the key and the buttons end up at. */
export async function confirmAir(ask: AirAsk): Promise<void> {
  studio.airAsk = null;
  studio.armed = null;
  studio.handoverTo = null;
  if (preparing()) return nudge();
  try {
    if (ask === 'end') {
      const r = await api.endShow();
      takeView(r.view);
      const n = endShowNote(r);
      say('good', n.title, n.message, 12000);
      return;
    }
    takeView(await api.setMode(ask));
    const n = modeNote(ask);
    say('good', n.title, n.message, 8000);
  } catch (e) {
    refused(e);
  }
}

// ---- the chat (M6): the same actions for a key and its button ----

type ChatAction = Extract<
  Action,
  {
    type:
      | 'mod-hide'
      | 'mod-timeout'
      | 'ban'
      | 'mod-leave'
      | 'mod-release'
      | 'release-all'
      | 'hide-all'
      | 'acknowledge'
      | 'escalate'
      | 'feature'
      | 'pin'
      | 'invite'
      | 'reply'
      | 'pass'
      | 'newest'
      | 'post'
      | 'avatar-remove'
      | 'attachment-remove'
      | 'sor-then'
      | 'sor-reason'
      | 'sor-send'
      | 'sor-cancel';
  }
>;

type Zone = 'needs' | 'studio' | 'room' | null;

/** The chat panel that has focus, if any: a key acts on that panel's selection first. */
function zone(): Zone {
  const p = document.activeElement?.closest('[data-panel]')?.getAttribute('data-panel');
  if (p === 'screener.needs') return 'needs';
  if (p === 'screener.studio') return 'studio';
  if (p === 'screener.room') return 'room';
  return null;
}

/** ↑ ↓ in a chat panel move its selection (and focus) instead of the caller queue's. */
function moveInPanel(delta: number): boolean {
  const z = zone();
  if (z === 'needs') {
    const key = step(entries().map((e) => e.key), needsEntry()?.key ?? null, delta);
    if (key) {
      chat.sel.needs = key;
      focusSoon(`[data-chat-key="${key}"] button`);
    }
    return true;
  }
  if (z === 'room') {
    const ids = chat.room.lines.filter((m) => !chat.room.hidden[m.id]).map((m) => String(m.id));
    const at = chat.sel.room === null ? null : String(chat.sel.room);
    const id = at === null && delta < 0 ? (ids.at(-1) ?? null) : step(ids, at, delta);
    if (id) {
      chat.sel.room = Number(id);
      focusSoon(`[data-chat="room-line"][data-id="${id}"]`);
    }
    return true;
  }
  if (z === 'studio') {
    const ids = chat.mod.studio.map((s) => String(s.m.id));
    const id = step(ids, studioMsg() ? String(studioMsg()!.m.id) : null, delta);
    if (id) {
      chat.sel.studio = Number(id);
      focusSoon(`[data-chat="studio-msg"][data-id="${id}"] button`);
    }
    return true;
  }
  return false;
}

/** Focus an element once the page has drawn the change that brings it. */
function focusSoon(selector: string): void {
  void tick().then(() => {
    const el = document.querySelector<HTMLElement>(selector);
    el?.focus();
    el?.scrollIntoView({ block: 'nearest' });
  });
}

/** What a per-message key (H O ⇧B) acts on: the panel with focus first, else the selected
 *  entry in Needs you, else the selected line in the room. */
function messageTarget(): { entry: Entry } | { line: Msg } | null {
  const z = zone();
  const entry = needsEntry();
  const line = roomLine();
  if (z === 'room') return line ? { line } : null;
  if (z === 'needs') return entry ? { entry } : null;
  if (entry) return { entry };
  return line ? { line } : null;
}

const NOTHING = (): void => chatSay('good', 'Nothing selected', 'Select a message in the room, or an item in Needs you, first.');

/** Where focus goes back to when the statement closes. */
let returnFocus: HTMLElement | null = null;

function targetOf(m: Msg, report: string | null, count = 0): SorTarget {
  return { id: m.id, name: m.author.name, at_ms: m.server_ms, body: m.body, report, count };
}

/** Open the statement of reasons ("Hide and tell them why"). */
export function openStatement(
  target: SorTarget | null,
  o: { then?: SorThen; reason?: Reason | null; automated?: string | null; bulk?: number | null } = {},
): void {
  const el = document.activeElement;
  returnFocus = el instanceof HTMLElement && el !== document.body ? el : null;
  studio.armed = null;
  chat.sor = {
    subject: 'message',
    picture: null,
    bulk: o.bulk ?? null,
    target,
    then: o.then ?? 'none',
    reason: o.reason ? sorReasonOf(o.reason) : null,
    note: '',
    automated: o.automated ?? null,
    disclose: !!o.automated,
    problem: '',
    sending: false,
  };
}

/** Open the statement of reasons for a chatter's picture (ADR-0235): refuse the one waiting, or
 *  remove the approved one. The same dialog, with no time-out or ban to add. */
export function openPictureStatement(picture: SorPicture, reason: Reason | null = null): void {
  openStatement(null, { reason });
  if (!chat.sor) return;
  chat.sor.subject = picture.waiting ? 'avatar-refuse' : 'avatar-remove';
  chat.sor.picture = picture;
}

function closeStatement(): void {
  chat.sor = null;
  const back = returnFocus;
  returnFocus = null;
  requestAnimationFrame(() => {
    if (back?.isConnected) back.focus();
    else
      (
        document.querySelector<HTMLElement>('[data-panel="screener.needs"] .item[aria-current="true"] button') ??
        document.querySelector<HTMLElement>('[data-panel="screener.needs"] h2')
      )?.focus();
  });
}

/** Hide, time out or ban from whatever is selected; `then` is what the key adds to a hide. */
function actOnMessage(then: SorThen): void {
  const t = messageTarget();
  if (!t) return NOTHING();
  if ('line' in t) return openStatement(targetOf(t.line, null), { then });
  const e = t.entry;
  if (e.kind === 'avatar') {
    if (then !== 'none') return chatSay('good', 'Not for a picture', 'Approve it (⏎), or refuse it and tell them why (H).');
    return openPictureStatement({
      id: e.avatar.id,
      waiting: true,
      account: e.avatar.account.id,
      name: e.avatar.account.name,
      report: null,
      kind: e.avatar.kind,
      attachment: e.avatar.attachment,
    });
  }
  if (e.kind === 'held') {
    return openStatement(targetOf(e.held.m, null), { then, automated: e.held.flagged ? e.held.flagged : null });
  }
  if (e.kind === 'notice') return chatSay('good', 'Not for a notice', 'Acknowledge it (A), or preserve the evidence and escalate it (⇧E).');
  if (e.kind === 'review') return chatSay('good', 'Not for a review', 'Keep the decision and close it (D).');
  const item = e.item;
  if (then === 'none') {
    if (!item.message) return chatSay('good', 'The message has gone', 'It is no longer in the room. Time out or ban the account instead.');
    return openStatement(targetOf(item.message, item.id, item.count), { reason: item.reason, automated: item.automated });
  }
  // A report already carries a reason: the time-out and the ban go at once (the ban took two presses).
  if (!item.account) return chatSay('good', 'No account to act on', 'This report names no account.');
  const account = item.account.id;
  void (then === 'timeout'
    ? mod({ op: 'timeout', account, minutes: TIMEOUT_MIN, reason: item.reason, report: item.id })
    : mod({ op: 'ban', account, reason: item.reason, report: item.id }));
}

/** Send the open reply field's words privately to the chatter (ADR-0040). */
export async function sendReply(): Promise<void> {
  const id = chat.replyFor;
  const body = chat.replyText.trim();
  if (id === null || !body) return;
  if (await mod({ op: 'reply', id, body })) {
    chat.replied[id] = body;
    chat.replyFor = null;
    chat.replyText = '';
    focusSoon(`[data-chat="studio-msg"][data-id="${id}"] [data-chat="studio-reply"]`);
  }
}

/** Close the reply field without sending. */
export function closeReply(): void {
  const id = chat.replyFor;
  chat.replyFor = null;
  chat.replyText = '';
  if (id !== null) focusSoon(`[data-chat="studio-msg"][data-id="${id}"] [data-chat="studio-reply"]`);
}

/** A review where the screener lifts the time-out or ban: lift it, then close the review. */
export async function liftAndClose(item: { id: string; account: { id: string } | null }): Promise<void> {
  if (!item.account) return;
  if (await mod({ op: 'lift', account: item.account.id })) void mod({ op: 'dismiss', report: item.id, note: 'Restriction lifted' });
}

function studioTarget(): ReturnType<typeof studioMsg> {
  const s = studioMsg();
  if (!s) chatSay('good', 'No messages to the studio', 'Nothing has come in privately yet.');
  return s;
}

function inOtherField(id: string): boolean {
  const el = document.activeElement;
  const field = el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && !['button', 'checkbox', 'radio'].includes(el.type));
  return field && el.id !== id;
}

async function chatAction(a: ChatAction): Promise<void> {
  switch (a.type) {
    case 'mod-hide':
      return actOnMessage('none');
    case 'mod-timeout':
      return actOnMessage('timeout');
    case 'ban':
      studio.armed = null;
      return actOnMessage('ban');
    case 'mod-leave': {
      const e = needsEntry();
      if (!e) return NOTHING();
      if (e.kind === 'report') void mod({ op: 'dismiss', report: e.item.id });
      else if (e.kind === 'review') void mod({ op: 'dismiss', report: e.item.id, note: 'Decision kept' });
      else if (e.kind === 'held') chatSay('good', 'It releases by itself', 'Release it now (⏎), or hide it (H).');
      else if (e.kind === 'avatar') chatSay('good', 'It waits for you', 'Approve the picture (⏎), or refuse it and tell them why (H).');
      else chatSay('good', 'Not for a notice', 'Acknowledge it (A), or preserve the evidence and escalate it (⇧E).');
      return;
    }
    case 'mod-release': {
      const e = needsEntry();
      if (e?.kind === 'held') void mod({ op: 'release', id: e.held.m.id });
      else if (e?.kind === 'avatar') void mod({ op: 'avatar_approve', avatar: e.avatar.id });
      else if (chat.mod.held.length) chatSay('good', 'Select the held message first', 'Click it, or use ↑ ↓ in Needs you.');
      return;
    }
    case 'release-all':
      if (chat.mod.held.length) void mod({ op: 'release_all' });
      return;
    case 'hide-all':
      if (chat.mod.held.length) openStatement(null, { bulk: chat.mod.held.length, reason: 'spam' });
      return;
    case 'acknowledge':
    case 'escalate': {
      const e = needsEntry();
      if (!e || e.kind === 'held' || e.kind === 'avatar' || e.kind === 'review' || (a.type === 'acknowledge' && e.kind !== 'notice')) {
        return chatSay('good', 'Select a notice first', a.type === 'acknowledge' ? 'Acknowledge is for a notice from someone without an account.' : 'Escalate is for a notice or a report.');
      }
      void mod(a.type === 'acknowledge' ? { op: 'acknowledge', report: e.item.id } : { op: 'escalate', report: e.item.id });
      return;
    }
    case 'feature':
    case 'pin': {
      const line = roomLine();
      if (!line) return chatSay('good', 'Select a message first', 'Click a message in the room, or use ↑ ↓ there.');
      if (a.type === 'feature') void mod({ op: 'feature', id: line.id });
      else void mod(chat.room.pinned?.id === line.id ? { op: 'unpin' } : { op: 'pin', id: line.id });
      return;
    }
    case 'invite': {
      const z = zone();
      const line = roomLine();
      const id = z === 'room' || (z !== 'studio' && line) ? line?.id : studioTarget()?.m.id;
      if (id === undefined) return chatSay('good', 'Select a message first', 'Click a message in the room or To the studio.');
      void mod({ op: 'invite', id });
      return;
    }
    case 'reply': {
      const s = studioTarget();
      if (!s) return;
      chat.sel.studio = s.m.id;
      chat.replyFor = s.m.id;
      chat.replyText = '';
      focusSoon('#chat-reply');
      return;
    }
    case 'pass': {
      const s = studioTarget();
      if (s) void mod({ op: 'pass', id: s.m.id });
      return;
    }
    case 'newest':
      chat.jump++;
      return;
    case 'avatar-remove':
      return openPictureStatement({ id: a.picture, waiting: false, account: a.account, name: a.name, report: a.report ?? null });
    case 'attachment-remove': {
      // The report's own copy of the message has the attachment to show; the room's line too.
      const line = chat.room.lines.find((m) => m.attachment?.id === a.attachment);
      const item = chat.mod.items.find((i) => i.attachment === a.attachment);
      const attachment = line?.attachment ?? item?.message?.attachment ?? undefined;
      return openPictureStatement({ id: a.attachment, waiting: false, account: a.account, name: a.name, report: a.report ?? null, kind: a.kind, attachment });
    }
    case 'post': {
      if (inOtherField('chat-compose')) return;
      const body = chat.compose.trim();
      if (!body) {
        document.getElementById('chat-compose')?.focus();
        return;
      }
      if (await mod({ op: 'post', body, pin: chat.composePin || undefined })) {
        chat.compose = '';
        chat.composePin = false;
      }
      return;
    }
    case 'sor-then':
      if (chat.sor && chat.sor.bulk === null && chat.sor.subject === 'message') chat.sor.then = a.then;
      return;
    case 'sor-reason':
      // The reason for suspected child sexual abuse material is for a picture only (ADR-0257).
      if (chat.sor && (a.reason !== 'suspected_csam' || chat.sor.picture)) {
        chat.sor.reason = a.reason;
        chat.sor.problem = '';
      }
      return;
    case 'sor-cancel':
      closeStatement();
      return;
    case 'sor-send': {
      const s = chat.sor;
      if (!s || s.sending) return;
      if (!s.reason) {
        s.problem = `Choose why first: ${sorReasonsFor(!!s.picture).map((r) => r.key).join(' ')}.`;
        return;
      }
      s.sending = true;
      const reason = s.reason;
      const words = reasonLabel(reason, chat.info);
      let ok: boolean;
      if (s.picture) {
        const note = s.note.trim() || undefined;
        const posted = s.picture.kind === 'picture' || s.picture.kind === 'document';
        ok = await mod(
          s.picture.waiting
            ? { op: 'avatar_refuse', avatar: s.picture.id, reason, note }
            : posted
              ? { op: 'attachment_remove', attachment: s.picture.id, reason, note, report: s.picture.report ?? undefined }
              : { op: 'avatar_remove', account: s.picture.account, reason, note, report: s.picture.report ?? undefined },
        );
      } else if (s.bulk !== null || !s.target) {
        ok = await mod({ op: 'hide_all_held', reason });
      } else {
        noteMyHide(s.target.id, words);
        ok = await mod({
          op: 'hide',
          id: s.target.id,
          reason,
          then: s.then,
          note: s.note.trim() || undefined,
          automated_disclosed: s.disclose,
          report: s.target.report ?? undefined,
        });
      }
      if (ok) closeStatement();
      else if (chat.sor) chat.sor.sending = false;
      return;
    }
  }
}
