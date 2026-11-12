// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The listener's chat socket (M6): one WebSocket to /api/v1/chat/socket on the page's own
// origin. The first frame is `hello`, with the session if signed in (never in the URL) and, on a
// reconnect, the last public position seen, so the server can send what was missed (ADR-0031).
// It reconnects after 1, 2, 4, 8, then 15 s; at once when the server closed it for falling
// behind (4008) or signed it out (4001). It pings every 25 s and keeps the server's clock skew
// for "sync chat to my stream" (ADR-0038).

import {
  CLOSE_SIGNED_OUT,
  CLOSE_SLOW,
  applyEvent,
  withAvatar,
  type ClientFrame,
  type Me,
  type MessageKind,
  type Msg,
  type Position,
  type RoomRules,
  type ServerFrame,
  type Statement,
} from '../../../../../packages/ui/chat.ts';
import { chatBackoff, heldCopy, positionAfter, socketUrl } from './model.ts';
import { attach as attachApi } from './api.ts';

export type Link = 'connecting' | 'open' | 'reconnecting';

/** One line in the private lane between this chatter and the studio (ADR-0040). */
export interface LaneEntry {
  id: string;
  at_ms: number;
  /** 'you', or the studio's name for itself. */
  from: string;
  mine: boolean;
  body: string;
  invite: boolean;
}

export type PostKind = Exclude<MessageKind, 'announcement'>;

export type AckResult =
  | { state: 'visible'; id: number | null }
  | { state: 'held'; id: number | null }
  | { state: 'refused'; words: string; retry_ms: number | null; code: string | null };

interface Pending {
  body: string;
  kind: PostKind;
  reply_to: Msg['reply_to'];
  resolve: (r: AckResult) => void;
}

const CAP = 500;
const PING_MS = 25_000;

export class ChatClient {
  link = $state<Link>('connecting');
  /** Room messages, oldest first, at most 500. */
  list = $state<Msg[]>([]);
  /** The chatter's own held messages, kept here until released or hidden. */
  held = $state<Msg[]>([]);
  pinned = $state<Msg | null>(null);
  /** The message last put on air, with when this page heard of it (local ms). */
  featured = $state<{ m: Msg; at: number } | null>(null);
  count = $state(0);
  rules = $state<RoomRules | null>(null);
  me = $state<Me | null>(null);
  statements = $state<Statement[]>([]);
  lane = $state<LaneEntry[]>([]);
  station = $state('');
  show = $state('');
  listenerDelayMs = $state(15_000);
  /** Server clock minus this page's clock, ms. */
  skewMs = $state(0);
  /** A frame the server could not take, in its words. */
  problem = $state<string | null>(null);
  session = $state<string | null>(null);
  /** The socket has been welcomed at least once since the page loaded. */
  everOpen = $state(false);
  /** Pictures this page attached, by attachment id: a preview from the file itself while the
   *  station decides (the station serves a waiting one only to its author, with the bearer). */
  previews = $state<Record<string, string>>({});

  /** New room messages as they arrive live (not history), for announcements. */
  onNew: ((m: Msg) => void) | null = null;
  /** A private line from the studio. */
  onPrivate: ((e: LaneEntry) => void) | null = null;
  /** The server signed this session out. */
  onSignedOut: (() => void) | null = null;

  #ws: WebSocket | null = null;
  #pos: Position | null = null;
  #attempt = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #ping: ReturnType<typeof setInterval> | undefined;
  #stopped = false;
  #pending = new Map<string, Pending>();
  #seq = 0;

  constructor(session: string | null) {
    this.session = session;
  }

  start(): void {
    this.#stopped = false;
    this.#open();
  }

  stop(): void {
    this.#stopped = true;
    clearTimeout(this.#timer);
    clearInterval(this.#ping);
    this.#failPending();
    this.#ws?.close(1000);
    this.#ws = null;
  }

  /** Sign in or out: reopen the socket with the new session, recovering where it was. */
  setSession(token: string | null): void {
    this.session = token;
    if (!token) {
      this.me = null;
      this.statements = [];
      this.lane = [];
      this.held = [];
    }
    this.#restart();
  }

  /** Send a post; resolves with the server's answer. */
  post(kind: PostKind, body: string, replyTo: Msg['reply_to'], playhead_ms: number | undefined): Promise<AckResult> {
    const ws = this.#ws;
    if (!ws || ws.readyState !== WebSocket.OPEN || this.link !== 'open') {
      return Promise.resolve({ state: 'refused', words: 'Not sent: the chat is reconnecting. Try again in a moment.', retry_ms: null, code: null });
    }
    const nonce = `${Date.now().toString(36)}-${++this.#seq}`;
    const frame: ClientFrame = { t: 'post', nonce, kind, body };
    if (replyTo) frame.reply_to = replyTo.id;
    if (playhead_ms !== undefined) frame.playhead_ms = playhead_ms;
    return new Promise((resolve) => {
      this.#pending.set(nonce, { body, kind, reply_to: replyTo, resolve });
      this.#send(frame);
    });
  }

  /** Post a message with a picture or document (ADR-0262). It waits for the station, shown to
   *  this chatter as waiting; a picture keeps a preview from the file chosen, on this page only. */
  async attach(file: File, body: string, replyTo: Msg['reply_to'], playhead_ms: number | undefined): Promise<AckResult> {
    if (!this.session || !this.me) return { state: 'refused', words: 'Sign in to post.', retry_ms: null, code: 'signed_out' };
    const nonce = `${Date.now().toString(36)}-${++this.#seq}`;
    try {
      const r = await attachApi(this.session, file, { nonce, name: file.name, body, reply_to: replyTo?.id, playhead_ms });
      if (r.attachment.kind === 'picture') this.previews[r.attachment.id] = URL.createObjectURL(file);
      if (!this.held.some((m) => m.id === r.id) && !this.list.some((m) => m.id === r.id)) {
        const author = { id: this.me.id, name: this.me.name, standing: this.me.standing, guest: false, avatar: this.me.avatar ?? null };
        this.held = [...this.held, { ...heldCopy(r.id, body, 'chat', author, replyTo, Date.now() + this.skewMs), attachment: r.attachment }];
      }
      return { state: 'held', id: r.id };
    } catch (e) {
      const status = (e as { status?: number }).status ?? 0;
      return { state: 'refused', words: e instanceof Error ? e.message : String(e), retry_ms: status === 429 ? 5_000 : null, code: null };
    }
  }

  react(id: number, emoji: string): void {
    this.#send({ t: 'react', id, emoji });
  }

  #send(f: ClientFrame): void {
    if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(JSON.stringify(f));
  }

  #restart(): void {
    clearTimeout(this.#timer);
    const old = this.#ws;
    this.#ws = null;
    if (old) {
      old.onclose = null;
      old.close(1000);
    }
    clearInterval(this.#ping);
    this.#failPending();
    this.#attempt = 0;
    if (!this.#stopped) this.#open();
  }

  #open(): void {
    this.link = this.#pos ? 'reconnecting' : 'connecting';
    let ws: WebSocket;
    try {
      ws = new WebSocket(socketUrl(location));
    } catch {
      this.#later();
      return;
    }
    this.#ws = ws;
    ws.onopen = () => {
      const hello: ClientFrame = { t: 'hello' };
      if (this.session) hello.session = this.session;
      if (this.#pos) hello.since = { ...this.#pos };
      ws.send(JSON.stringify(hello));
      clearInterval(this.#ping);
      this.#ping = setInterval(() => this.#send({ t: 'ping' }), PING_MS);
    };
    ws.onmessage = (ev) => {
      let f: ServerFrame;
      try {
        f = JSON.parse(String(ev.data)) as ServerFrame;
      } catch {
        return;
      }
      this.#frame(f);
    };
    ws.onclose = (ev) => {
      if (this.#ws !== ws) return;
      this.#ws = null;
      clearInterval(this.#ping);
      this.#failPending();
      if (this.#stopped) return;
      this.link = 'reconnecting';
      if (ev.code === CLOSE_SIGNED_OUT) {
        this.setSession(null);
        this.onSignedOut?.();
        return;
      }
      if (ev.code === CLOSE_SLOW) {
        this.#attempt = 0;
        this.#open();
        return;
      }
      this.#later();
    };
  }

  #later(): void {
    this.link = 'reconnecting';
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#open(), chatBackoff(this.#attempt++));
  }

  #failPending(): void {
    for (const p of this.#pending.values()) {
      p.resolve({ state: 'refused', words: 'Not sent: the chat lost its connection. Try again in a moment.', retry_ms: null, code: null });
    }
    this.#pending.clear();
  }

  #frame(f: ServerFrame): void {
    switch (f.t) {
      case 'welcome': {
        this.#attempt = 0;
        this.skewMs = f.server_now_ms - Date.now();
        // A stored session the server no longer knows: forget it rather than resend it.
        const stale = this.session !== null && f.me === null;
        this.station = f.station;
        this.show = f.show;
        this.me = f.me;
        this.rules = f.rules;
        this.listenerDelayMs = f.listener_delay_ms;
        this.count = f.count;
        this.statements = f.statements;
        this.problem = null;
        if (f.recovered && this.#pos) {
          this.#pos = { epoch: f.epoch, offset: this.#pos.offset };
          for (const e of f.missed) this.#frame(e);
        } else {
          this.list = f.history.slice(-CAP);
          this.pinned = f.pinned;
        }
        this.#pos = { epoch: f.epoch, offset: Math.max(f.offset, f.recovered && this.#pos ? this.#pos.offset : 0) };
        this.link = 'open';
        this.everOpen = true;
        if (stale) {
          this.session = null;
          this.onSignedOut?.();
        }
        return;
      }
      case 'msg':
      case 'hide':
      case 'pin':
      case 'feature':
      case 'react':
      case 'avatar':
        this.#public(f);
        this.#pos = positionAfter(this.#pos, f);
        return;
      case 'count':
        this.count = f.n;
        return;
      case 'rules':
        this.rules = f.rules;
        this.show = f.show;
        this.listenerDelayMs = f.listener_delay_ms;
        return;
      case 'ack':
        this.#ack(f);
        return;
      case 'you':
        this.me = f.me;
        return;
      case 'statement': {
        const rest = this.statements.filter((s) => s.id !== f.s.id);
        this.statements = [...rest, f.s];
        return;
      }
      case 'private': {
        const e: LaneEntry = { id: `p-${f.id}`, at_ms: f.at_ms, from: f.from, mine: false, body: f.body, invite: f.invite };
        this.#lane(e);
        this.onPrivate?.(e);
        return;
      }
      case 'mine':
        // A message with a picture or document waits for the station (ADR-0262): it shows in the
        // room as the chatter's own, waiting, not in the lane to the studio.
        if (f.m.attachment) {
          if (!this.held.some((m) => m.id === f.m.id) && !this.list.some((m) => m.id === f.m.id)) this.held = [...this.held, f.m];
          return;
        }
        this.#lane({ id: `m-${f.m.id}`, at_ms: f.m.server_ms, from: 'you', mine: true, body: f.m.body, invite: false });
        return;
      case 'released': {
        const ids = new Set(f.ids);
        this.held = this.held.map((m) => (ids.has(m.id) ? { ...m, state: 'visible' } : m));
        return;
      }
      case 'rejected': {
        // A moderator turned a held message down: it never reached the room. The statement of
        // reasons arrives beside this.
        const ids = new Set(f.ids);
        this.held = this.held.filter((m) => !ids.has(m.id));
        return;
      }
      case 'error':
        this.problem = f.words;
        return;
      default:
        return;
    }
  }

  #public(f: Extract<ServerFrame, { offset: number }>): void {
    const isNew = f.t === 'msg' && !this.list.some((m) => m.id === f.m.id);
    this.list = applyEvent(this.list, f, CAP);
    if (f.t === 'msg') {
      // Our own held copy gives way to the room's message.
      if (this.held.some((m) => m.id === f.m.id)) this.held = this.held.filter((m) => m.id !== f.m.id);
      if (isNew) this.onNew?.(f.m);
    } else if (f.t === 'hide') {
      const gone = new Set(f.ids);
      this.held = this.held.filter((m) => !gone.has(m.id));
      if (this.pinned && gone.has(this.pinned.id)) this.pinned = null;
      if (this.featured && gone.has(this.featured.m.id)) this.featured = null;
    } else if (f.t === 'pin') {
      this.pinned = f.m;
    } else if (f.t === 'feature') {
      this.featured = { m: f.m, at: Date.now() };
    } else if (f.t === 'react' && this.pinned?.id === f.id) {
      this.pinned = { ...this.pinned, reactions: f.reactions };
    } else if (f.t === 'avatar') {
      // Every copy of that chatter's messages takes the new avatar (ADR-0235).
      const mine = (m: Msg) => (m.author.id === f.account ? withAvatar(m, f.avatar) : m);
      this.held = this.held.map(mine);
      if (this.pinned) this.pinned = mine(this.pinned);
      if (this.featured) this.featured = { ...this.featured, m: mine(this.featured.m) };
    }
  }

  #ack(f: Extract<ServerFrame, { t: 'ack' }>): void {
    const p = this.#pending.get(f.nonce);
    if (!p) return;
    this.#pending.delete(f.nonce);
    if (f.state === 'refused') {
      p.resolve({ state: 'refused', words: f.words ?? 'Not sent.', retry_ms: f.retry_ms, code: f.code });
      return;
    }
    if (f.state === 'held' && f.id !== null && p.kind !== 'studio' && this.me) {
      const author = { id: this.me.id, name: this.me.name, standing: this.me.standing, guest: this.me.kind === 'guest', avatar: this.me.avatar ?? null };
      if (!this.list.some((m) => m.id === f.id)) {
        this.held = [...this.held, heldCopy(f.id, p.body, p.kind, author, p.reply_to, Date.now() + this.skewMs)];
      }
    }
    p.resolve({ state: f.state, id: f.id });
  }

  #lane(e: LaneEntry): void {
    if (this.lane.some((x) => x.id === e.id)) return;
    this.lane = [...this.lane, e].sort((a, b) => a.at_ms - b.at_ms);
  }
}
