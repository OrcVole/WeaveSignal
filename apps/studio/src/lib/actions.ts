// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What each key (and each button, which runs the same action) does: the keymap's actions
// turned into the control plane's commands. One place, so a button and its key never differ.

import { tick } from 'svelte';
import * as api from './api.ts';
import { cartKey, type Action } from '../../../../packages/ui/keymap.ts';
import { cartAt, cartNow, moveSlot } from '../../../../packages/ui/carts.ts';
import { say, setBank, stationNow, studio, takeView } from './studio.svelte.ts';
import { moveWeek, today } from './schedule.svelte.ts';
import { firstName } from './format.ts';
import { handoverStep } from './handover.ts';
import { chat, chatSay, entries, mod, needsEntry, noteMyHide, roomLine, studioMsg, type SorPicture, type SorTarget } from './chat.svelte.ts';
import { TIMEOUT_MIN, reasonLabel, sorReasonOf, sorReasonsFor, step, type Entry } from './chatModel.ts';
import type { Msg, Reason } from '../../../../packages/ui/chat.ts';
import type { SorThen } from '../../../../packages/ui/keymap.ts';

const NOT_BUILT: Partial<Record<Action['type'], [string, string]>> = {
  'restart-playout': ['Restart is not wired yet', 'Restart Liquidsoap from the server for now; ⌃R will do it from here.'],
  'restart-encoder': ['Not wired yet', 'The encoder cannot be restarted from here yet; restart the listener process on the server.'],
  'backup-stream': ['Not wired yet', 'Switching listeners to a backup stream is not built yet.'],
  'end-show': ['Not wired yet', 'Ending a show from here is not built yet.'],
};

/** A cart's key, its button, the desk's glance and F1 to F6 all come here: fire the cart in
 *  that slot of the current bank, or fade it if it is sounding (keymap section 4). With `cue`
 *  (⇧ and the key) it goes into the presenter's headphones only, or comes out of them. */
export async function fireSlot(slot: number, cue = false): Promise<void> {
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
  return p ? firstName(p.name, identity) : identity;
}

async function send(p: Promise<api.View>): Promise<boolean> {
  try {
    takeView(await p);
    return true;
  } catch (e) {
    say('warning', 'Not done', e instanceof Error ? e.message : String(e));
    return false;
  }
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

/** The desk's toast that Esc answers: a caller sent and waiting for the presenter. */
export function readyCaller() {
  const v = studio.view;
  return v?.sources.find((s) => s.role === 'caller' && s.caller?.sent && !s.caller.taken && !s.on_air && !s.cue) ?? null;
}

export async function run(a: Action): Promise<void> {
  const v = studio.view;
  const now = Date.now();
  switch (a.type) {
    case 'tab':
      if (a.tab === 'desk' || a.tab === 'screener' || a.tab === 'carts' || a.tab === 'schedule') {
        studio.tab = a.tab;
        document.documentElement.dataset.surface = a.tab;
      } else say('good', 'Not in this build', `The ${a.tab} tab is not in this build yet.`);
      return;
    case 'arm':
      studio.armed = { what: a.what, at: now, identity: a.identity };
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
      if (studio.note) {
        studio.note = null;
        return;
      }
      if (chat.toast && chat.toast.until > Date.now()) {
        chat.toast = null;
        return;
      }
      if (studio.tab === 'desk') {
        const c = readyCaller();
        if (c) {
          await send(api.screener({ op: 'decline', identity: c.identity, reason: 'not now' }));
          return;
        }
      }
      // On the Carts tab Esc takes every cued cart out of the headphones (cart-wall.md §4).
      if (studio.tab === 'carts' && v?.carts.sounding.some((s) => s.cue && !s.stopping)) {
        await send(api.carts({ op: 'clear_cue' }));
        return;
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
      if (await send(api.endPass({ identity: a.identity }))) {
        say('good', 'Pass ended', `${who} is out of the room and their link no longer works. Send a new invite if they are to come back.`);
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
    case 'influx':
      studio.armed = null;
      if (v) await send(api.chat({ op: 'influx', on: !v.chat.influx }));
      return;
    case 'handover': {
      if (!v) return;
      const step = handoverStep(v.sources, v.seats.presenter, studio.handoverTo);
      if (step.kind === 'none') {
        studio.handoverTo = null;
        return void say('good', 'No DJ to hand over to', 'Invite the next DJ with Invite someone…, beside Not on air, keeping the role DJ. They wait there off air; ⇧⏎ then brings them on air with you.');
      }
      const dj = firstName(step.dj.name, step.dj.identity);
      if (step.kind === 'done') {
        studio.handoverTo = null;
        return void say('good', `${dj} has the air`, `The handover is done: ${dj} holds the air and the desk's talk keys.`);
      }
      if (step.kind === 'on') {
        if (await send(api.desk({ op: 'on_air', identity: step.dj.identity, on: true }))) {
          studio.handoverTo = step.dj.identity;
          say('good', `${dj} is on air with you`, `The overlap: say hello. Press ⇧⏎ again to take yourself off and hand ${dj} the air.`, 15000);
        }
        return;
      }
      for (const s of step.off) if (!(await send(api.desk({ op: 'on_air', identity: s.identity, on: false })))) return;
      studio.handoverTo = null;
      const off = step.off.map((s) => firstName(s.name, s.identity)).join(' and ');
      say('good', `Handed over to ${dj}`, `${off} off air; ${dj} holds the air. This window's talk keys now speak as ${dj}: close it, or hand it to them, and close your invite page when you leave (ADR-0239).`, 12000);
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
      if (!s) return;
      const field = a.type === 'on' ? 'on_air' : a.type;
      await send(api.desk({ op: field, identity: s.identity, on: !s[field] }));
      return;
    }
    case 'talk': {
      const s = sourceAt(a.slot);
      const me = v?.seats.presenter;
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
      const id = studio.selected;
      const me = v?.seats.screener;
      if (!id || !me) return void (a.held && say('good', 'Nobody to talk to', !id ? 'Select a caller first (↑ ↓ or 1 to 9).' : 'Join the room as the screener.'));
      const c = v?.callers.find((x) => x.identity === id);
      if (a.held && c?.state === 'waiting') await send(api.screener({ op: 'screen', identity: id }));
      await send(api.desk({ op: 'talk_to', identity: me, to: id, on: a.held }));
      talkingTo(id, a.held);
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
    case 'week':
      moveWeek(a.delta);
      return;
    case 'today':
      today(stationNow());
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
    default: {
      const nb = NOT_BUILT[a.type];
      if (nb) say('good', nb[0], nb[1]);
    }
  }
}

export { name as personName };

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
