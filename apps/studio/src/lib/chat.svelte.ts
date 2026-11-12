// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The screener's link to the chat service (M6): a staff session swapped for the admin bearer,
// the socket with its recovery (`since`), the moderation snapshot kept from its frames, and the
// moderation commands. The contract is packages/ui/chat.ts.
//
// The chat's address is the page's own origin (the Vite proxy sends /api/v1/chat there, with
// the admin bearer added), or `?chat=` names another, or else the control plane's `?control=`;
// the bearer comes the way the control plane's does (api.ts: the proxy, or `?token=`).

import {
  CLOSE_SIGNED_OUT,
  CLOSE_SLOW,
  type ChatInfo,
  type Me,
  type ModCommand,
  type ModSnapshot,
  type Msg,
  type Position,
  type RoomRules,
  type ServerFrame,
  type SignedIn,
  type StudioMsg,
} from '../../../../packages/ui/chat.ts';
import type { SorReason, SorThen } from '../../../../packages/ui/keymap.ts';
import { base, headers, netFetch } from './api.ts';
import { studio } from './studio.svelte.ts';
import { account } from './account.svelte.ts';
import { EMPTY_MOD, EMPTY_ROOM, applyMod, applyRoom, needsList, roomFrom, type Entry, type Room, type SorSubject } from './chatModel.ts';

/** `none`: the station has no chat (its chat routes answer 404), checked again now and then. */
export type ChatLink = 'off' | 'connecting' | 'live' | 'lost' | 'none';

/** How often a station with no chat is asked again, in case one has been added (issue 14). */
export const NO_CHAT_RETRY_MS = 5 * 60_000;

/** A refused chat request, with its HTTP status. */
export class ChatHttpError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** What the statement of reasons acts on: one message, or every held message at once. */
export interface SorTarget {
  id: number;
  name: string;
  at_ms: number;
  body: string;
  /** The report it answers, if any. */
  report: string | null;
  count: number;
}

/** The picture a statement of reasons is about (ADR-0235), or a picture or document posted in the
 *  chat (ADR-0262). */
export interface SorPicture {
  /** The picture's id. */
  id: string;
  /** An avatar (absent), or a posted picture or document. */
  kind?: 'avatar' | 'picture' | 'document';
  /** A posted one, to show in the dialog. */
  attachment?: import('../../../../packages/ui/chat.ts').Attachment;
  /** Waiting for approval (it is being refused); false for an approved one being removed. */
  waiting: boolean;
  account: string;
  name: string;
  /** The report that led here, if one did. */
  report: string | null;
}

export interface Sor {
  /** A message (the default), or a chatter's picture refused or removed. */
  subject: SorSubject;
  picture: SorPicture | null;
  /** Every held message (⇧X): the number when opened; null for one message. */
  bulk: number | null;
  target: SorTarget | null;
  then: SorThen;
  reason: SorReason | null;
  note: string;
  /** The automated step involved, if one was ('rustrict'): then the disclosure is fixed on. */
  automated: string | null;
  disclose: boolean;
  /** Why ⏎ did nothing, said beside the button. */
  problem: string;
  sending: boolean;
}

export interface ChatToast {
  id: number;
  tone: 'good' | 'warning';
  title: string;
  msg: string;
  until: number;
}

export const chat = $state({
  link: 'off' as ChatLink,
  why: '',
  /** When the next try starts, on this machine's clock. */
  retryAt: 0,
  me: null as Me | null,
  station: '',
  show: '',
  rules: null as RoomRules | null,
  /** People reading the room. */
  count: 0,
  /** The chat service's clock minus this machine's. */
  skew: 0,
  mod: EMPTY_MOD as ModSnapshot,
  /** The session got the moderation snapshot (a staff or moderator session). */
  canModerate: true,
  room: EMPTY_ROOM as Room,
  /** Arrival times of new room messages in the last minute, for the rate. */
  arrivals: [] as number[],
  info: null as ChatInfo | null,
  sel: { needs: null as string | null, room: null as number | null, studio: null as number | null },
  sor: null as Sor | null,
  /** Where an armed Ban will land, for its "Press again" button. */
  banAt: 'needs' as 'needs' | 'room',
  /** The studio message whose reply field is open. */
  replyFor: null as number | null,
  replyText: '',
  /** Replies this window sent, until the service's own copy arrives. */
  replied: {} as Record<number, string>,
  compose: '',
  composePin: false,
  /** Bumped by `End`: the room scrolls to its newest line. */
  jump: 0,
  toast: null as ChatToast | null,
  /** Chatters' own pictures as fetched with the studio's bearer, by id: an object URL, or null
   *  for one that could not be fetched (gone, or no chat behind the address). */
  pictures: {} as Record<string, string | null>,
  /** Pictures posted in the chat (ADR-0262), as fetched with the bearer: `<id>:thumb` and
   *  `<id>:full`, an object URL or null. */
  attachments: {} as Record<string, string | null>,
  /** The posted picture shown full size (screener.attachment), or null. */
  viewing: null as import('../../../../packages/ui/chat.ts').Attachment | null,
});

// ---- the address ----

export function chatBase(): string {
  const q = new URLSearchParams(globalThis.location?.search ?? '');
  return (q.get('chat') ?? '').replace(/\/$/, '') || base();
}

function socketUrl(): string {
  const origin = chatBase() || globalThis.location.origin;
  return `${origin.replace(/^http/, 'ws')}/api/v1/chat/socket`;
}

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await netFetch(`${chatBase()}${path}`, init);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ChatHttpError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as T;
}

// ---- the toast ----

let toastId = 0;

export function chatSay(tone: ChatToast['tone'], title: string, msg: string, ms = 6000): void {
  chat.toast = { id: ++toastId, tone, title, msg, until: Date.now() + ms };
}

// ---- the link ----

let started = false;
let session: string | null = null;
let since: Position | null = null;
let socket: WebSocket | null = null;
/** Hides this window sent, by message id, with the reason's words: the room marks them "by you". */
const myHides: Record<number, string> = {};

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** The name this window moderates under: the signed-in member of staff's own (ADR-0127; the
 *  chat service names them from the session itself), else the screener seat's name from the
 *  control plane's view once it has arrived (or "Screener"), as a station key has no name. */
async function myName(): Promise<string> {
  for (let i = 0; i < 30 && !account.loaded; i++) await sleep(100);
  if (account.me?.kind === 'staff' && account.me.account?.name) return account.me.account.name;
  for (let i = 0; i < 30 && !studio.view; i++) await sleep(100);
  return studio.view?.seats.screener_name || 'Screener';
}

/** The name this window acts as: the one given to /staff/session, sent as the actor on every
 *  moderation command (the proxy puts the station's bearer on them, so the bearer cannot say who). */
let actor = '';

/** The name the moderation log and the statement dialog show for this window. */
export function actorName(): string {
  return chat.me?.name || actor || studio.view?.seats.screener_name || 'Screener';
}

async function staffSession(): Promise<string> {
  actor = await myName();
  const s = await call<SignedIn>('/api/v1/chat/staff/session', {
    method: 'POST',
    headers: headers(true),
    body: JSON.stringify({ name: actor, role: 'screener' }),
  });
  chat.me = s.me;
  return s.session;
}

function modHeaders(): HeadersInit {
  // A header holds Latin-1 only: a name beyond it goes percent-encoded (fetch would throw).
  const name = actor || actorName();
  const safe = /^[\x20-\x7e\u00a0-\u00ff]*$/.test(name) ? name : encodeURIComponent(name);
  return { ...(headers(true) as Record<string, string>), 'X-WeaveSignal-Actor': safe };
}

async function loadInfo(): Promise<void> {
  try {
    chat.info = await call<ChatInfo>('/api/v1/chat/info');
  } catch {
    // The preview does without the service's own words.
  }
}

/** Start following the chat; it keeps going for as long as the page lives. */
export function startChat(): void {
  if (started) return;
  started = true;
  void run();
}

async function run(): Promise<void> {
  let attempt = 0;
  for (;;) {
    chat.link = 'connecting';
    let code = 0;
    try {
      session ??= await staffSession();
      if (!chat.info) void loadInfo();
      const out = await connect(session);
      code = out.code;
      if (out.welcomed) attempt = 0;
      if (code === CLOSE_SIGNED_OUT || out.refused) session = null;
      chat.why = out.why;
    } catch (e) {
      chat.why = e instanceof Error ? e.message : String(e);
      // No chat service behind the station's address (issue 14): say so quietly, not as a
      // fault, and look again now and then.
      if (e instanceof ChatHttpError && e.status === 404) {
        chat.link = 'none';
        chat.why = '';
        chat.retryAt = Date.now() + NO_CHAT_RETRY_MS;
        await sleep(NO_CHAT_RETRY_MS);
        continue;
      }
    }
    socket = null;
    // A client that fell behind reconnects at once and recovers (4008).
    if (code === CLOSE_SLOW) continue;
    chat.link = 'lost';
    const wait = Math.min(30_000, 1000 * 2 ** attempt++);
    chat.retryAt = Date.now() + wait;
    await sleep(wait);
  }
}

function connect(token: string): Promise<{ code: number; why: string; welcomed: boolean; refused: boolean }> {
  return new Promise((resolve) => {
    const ws = new WebSocket(socketUrl());
    socket = ws;
    let welcomed = false;
    let refused = false;
    let ping: ReturnType<typeof setInterval> | undefined;
    ws.onopen = () => {
      ws.send(JSON.stringify({ t: 'hello', session: token, ...(since ? { since } : {}) }));
      ping = setInterval(() => ws.readyState === WebSocket.OPEN && ws.send(JSON.stringify({ t: 'ping' })), 25_000);
    };
    ws.onmessage = (e) => {
      let f: ServerFrame;
      try {
        f = JSON.parse(String(e.data)) as ServerFrame;
      } catch {
        return;
      }
      if (f.t === 'welcome') welcomed = true;
      // An error before the welcome is the hello refused: a new session next time.
      if (f.t === 'error' && !welcomed) refused = true;
      take(f);
    };
    ws.onclose = (e) => {
      clearInterval(ping);
      resolve({ code: e.code, why: e.reason || (welcomed ? 'the chat closed the link' : 'the chat did not answer'), welcomed, refused });
    };
  });
}

function advance(offset: number): void {
  if (since && offset > since.offset) since = { ...since, offset };
}

/** One frame from the socket. */
export function take(f: ServerFrame): void {
  const now = Date.now();
  switch (f.t) {
    case 'welcome': {
      chat.link = 'live';
      chat.why = '';
      chat.skew = f.server_now_ms - now;
      chat.station = f.station;
      chat.show = f.show;
      chat.rules = f.rules;
      chat.count = f.count;
      chat.me = f.me;
      since = { epoch: f.epoch, offset: f.offset };
      if (f.recovered) {
        for (const m of f.missed) take(m);
      } else {
        chat.room = roomFrom(f.history, f.pinned);
        chat.arrivals = [];
      }
      chat.room = { ...chat.room, pinned: f.pinned };
      // Moderators' frames carry no offset: the snapshot is the truth on every welcome.
      chat.mod = f.moderation ?? EMPTY_MOD;
      chat.canModerate = f.moderation !== null;
      return;
    }
    case 'msg':
    case 'hide':
    case 'pin':
    case 'feature':
    case 'avatar':
    case 'react': {
      advance(f.offset);
      if (f.t === 'msg' && !chat.room.lines.some((m) => m.id === f.m.id)) {
        chat.arrivals = [...chat.arrivals.filter((t) => now - t <= 60_000), now];
      }
      chat.room = applyRoom(chat.room, f, now, myHides);
      return;
    }
    case 'mod_done':
      chat.room = applyRoom(chat.room, f, now, myHides);
      chatSay('good', `${f.by}, ${hmsOf(f.at_ms)}`, f.what);
      return;
    case 'count':
      chat.count = f.n;
      return;
    case 'rules':
      chat.rules = f.rules;
      chat.show = f.show;
      return;
    case 'mod_held':
    case 'mod_unheld':
    case 'mod_avatar':
    case 'mod_avatar_done':
    case 'mod_item':
    case 'mod_item_done':
    case 'mod_studio':
      chat.mod = applyMod(chat.mod, f);
      if (f.t === 'mod_studio' && f.s.reply !== null) delete chat.replied[f.s.m.id];
      return;
    case 'error':
      chatSay('warning', 'The chat refused that', f.words);
      return;
    default:
      return;
  }
}

function hmsOf(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** Stop and forget (tests and a page that unmounts for good). */
export function chatLinkClose(): void {
  socket?.close();
}

// ---- the commands ----

/** Send a moderation command. A refusal shows in the toast slot, in the service's words. */
export async function mod(cmd: ModCommand): Promise<boolean> {
  try {
    const out = await call<{ ok: boolean; done: string }>('/api/v1/chat/mod', {
      method: 'POST',
      headers: modHeaders(),
      body: JSON.stringify(cmd),
    });
    // The socket's `mod_done` reports it; without a socket, the answer does.
    if (chat.link !== 'live' && out.done) chatSay('good', 'Done', out.done);
    return true;
  } catch (e) {
    chatSay('warning', 'Not done', e instanceof Error ? e.message : String(e));
    return false;
  }
}

// ---- chatters' own pictures (ADR-0235) ----

const asked = new Set<string>();

/** A chatter's own picture, waiting or approved, as an object URL; undefined until it has been
 *  fetched. The studio fetches every one with its bearer (`/mod/avatars/{id}`): a held picture
 *  is for staff only, and behind Cloudron's login a plain <img> request never reaches the
 *  station. Kept for the life of the window; a picture's bytes never change under an id. */
export function pictureOf(id: string): string | null | undefined {
  if (!asked.has(id)) {
    asked.add(id);
    void (async () => {
      try {
        const res = await netFetch(`${chatBase()}/api/v1/chat/mod/avatars/${encodeURIComponent(id)}`, { headers: headers(false) });
        chat.pictures[id] = res.ok ? URL.createObjectURL(await res.blob()) : null;
      } catch {
        chat.pictures[id] = null;
      }
    })();
  }
  return chat.pictures[id];
}

// ---- pictures and documents posted in the chat (ADR-0262) ----

const askedFiles = new Set<string>();

/** A posted picture's thumbnail or full size as an object URL, fetched with the studio's bearer
 *  (a waiting one is for staff and moderators only); undefined until fetched, null if it cannot
 *  be. A picture's bytes never change under an id. */
export function attachmentOf(id: string, which: 'thumb' | 'full' = 'thumb'): string | null | undefined {
  const key = `${id}:${which}`;
  if (!askedFiles.has(key)) {
    askedFiles.add(key);
    void (async () => {
      try {
        const path = `/api/v1/chat/pictures/${encodeURIComponent(id)}${which === 'thumb' ? '/thumb' : ''}`;
        const res = await netFetch(`${chatBase()}${path}`, { headers: headers(false) });
        chat.attachments[key] = res.ok ? URL.createObjectURL(await res.blob()) : null;
      } catch {
        chat.attachments[key] = null;
      }
    })();
  }
  return chat.attachments[key];
}

/** Save a posted document to look at it before deciding: fetched with the bearer, saved under the
 *  station's own name (never the chatter's), never opened here. */
export async function saveAttachment(id: string): Promise<void> {
  const res = await netFetch(`${chatBase()}/api/v1/chat/files/${encodeURIComponent(id)}`, { headers: headers(false) });
  if (!res.ok) {
    const e = await res.json().catch(() => ({}));
    throw new Error(e.error ?? `${res.status} ${res.statusText}`);
  }
  const name = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') ?? '')?.[1] ?? `chat-document-${id.slice(0, 8)}`;
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/** Remember a hide this window sends, so its line reads "Hidden by you". */
export function noteMyHide(id: number, why: string): void {
  myHides[id] = why;
}

// ---- what the keys and buttons act on ----

/** "Needs you" in its order. */
export function entries(): Entry[] {
  return needsList(chat.mod);
}

/** The selected entry in "Needs you", or the first. */
export function needsEntry(): Entry | null {
  const list = entries();
  return list.find((e) => e.key === chat.sel.needs) ?? list[0] ?? null;
}

/** The selected line in the room (never a hidden one). */
export function roomLine(): Msg | null {
  const id = chat.sel.room;
  if (id === null || chat.room.hidden[id]) return null;
  return chat.room.lines.find((m) => m.id === id) ?? null;
}

/** The selected message to the studio, or the first. */
export function studioMsg(): StudioMsg | null {
  const list = chat.mod.studio;
  return list.find((s) => s.m.id === chat.sel.studio) ?? list[0] ?? null;
}

/** The chat's clock now (epoch ms), ticking with the studio clock. */
export function chatNow(): number {
  return studio.now + chat.skew;
}
