// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The live chat's wire types and routes (M6), shared by the player page (apps/web) and the
// studio app's screener (apps/studio). The server's side is crates/chat/src/wire.rs and
// crates/chat/src/http.rs: change both together.
//
// Everything is under /api/v1/chat/ on the station's own origin (the proxy sends that prefix
// to the chat service, never through the CDN). JSON in and out; a problem answers
// `{ "error": "<sentence>", "field"?: "<name>" }` with a 4xx or 5xx status.
//
// Credentials:
//  - A chatter's session token, `Authorization: Bearer <session>`, from sign-in or a guest name.
//    In the socket it goes in the first frame (`hello`), never in the URL.
//  - The studio app's routes take the station's admin bearer (the same one the control plane
//    takes, until accounts arrive); the app swaps it for a staff session for the socket.
//
// ---- public routes (no credential) ----
// GET  /api/v1/chat/info                 → ChatInfo: the station's legal fields, rules, reasons
// POST /api/v1/chat/signin/start         {email}                       → 202 {sent: true, expires_s}
// POST /api/v1/chat/signin/finish        {email, code, name?, age_ok?, terms_version?}
//                                        → 200 SignedIn, or 422 {error, field:'name'|'age_ok'|'terms'}
//                                          when a new account still needs a name, the age
//                                          declaration or the terms (`needs` lists which)
// POST /api/v1/chat/guest                {name, age_ok, terms_version} → SignedIn (only where guests are on)
// POST /api/v1/chat/notices              NoticeRequest (someone without an account, DSA Art. 16) → 201 {id}
// GET  /api/v1/chat/socket               the WebSocket
// GET  /api/v1/chat/avatars/{id}         a chatter's own picture, once staff approved it (image/png);
//                                        404 for one still held, refused or removed (ADR-0235)
// GET  /api/v1/chat/pictures/{id}        a picture posted in the chat, once approved and while its
//                                        message shows (PNG or JPEG, inline); /thumb its thumbnail
// GET  /api/v1/chat/files/{id}           a document posted in the chat, the same; always a download
//                                        (Content-Disposition: attachment) under the station's own name
//   Each takes a bearer too: the author sees their own waiting, staff and moderators any (ADR-0262).
//
// ---- a chatter's own (session bearer) ----
// GET  /api/v1/chat/me                   → {me: Me, statements: Statement[]}
// POST /api/v1/chat/signout              → 204
// POST /api/v1/chat/reports              ReportRequest → 201 {id}
// POST /api/v1/chat/statements/{id}/review {text}          → 201 {id}
// PUT  /api/v1/chat/me/avatar/stock      {id: string | null}  → {me}: one of the shipped avatars, or none
// PUT  /api/v1/chat/me/avatar            the picture itself (PNG or JPEG) as the body → 202 {me}; it is
//                                        held until staff approve it (not for guests)
// GET  /api/v1/chat/me/avatar            the chatter's own picture: the one held, else the approved one
// DELETE /api/v1/chat/me/avatar          → {me}: their own picture is deleted, held or approved
// POST /api/v1/chat/attachments?name=&body=&nonce=&reply_to=  the file itself as the body (a trusted
//                                        chatter, the switch on) → 202 {id, state: 'held', attachment};
//                                        held until staff or a moderator approve it (ADR-0262)
//
// ---- the studio (admin bearer, or a moderator's session) ----
// POST /api/v1/chat/staff/session        {name, role?}  → SignedIn (a staff session for the socket)
// POST /api/v1/chat/mod                  ModCommand     → 200 {ok: true, done: string}
// GET  /api/v1/chat/mod/log?limit=100    → {log: LogEntry[]}
// GET  /api/v1/chat/mod/accounts/{id}    → AccountDetail: the last 50 messages, restrictions, statements
// GET  /api/v1/chat/mod/avatars/{id}     any uploaded picture, a held one included (staff only)
// ---- station admin (admin bearer) ----
// GET  /api/v1/chat/admin/settings       → StationChatSettings;  PUT the same shape
// GET  /api/v1/chat/admin/accounts/{id}/export → the account's data as JSON (a download)
// POST /api/v1/chat/admin/accounts/{id}/erase  → 200 {erased: true, messages: n}
// GET  /api/v1/chat/admin/holds          → {holds: EvidenceHold[]}
// GET  /api/v1/chat/admin/risk-assessment → text/markdown: a template to start from

export type Standing = 'member' | 'trusted' | 'moderator' | 'staff';
export type AccountKind = 'email' | 'guest' | 'staff';
export type MessageKind = 'chat' | 'request' | 'studio' | 'announcement';
export type MsgState = 'visible' | 'held' | 'hidden';

export interface Position {
  epoch: string;
  offset: number;
}

/** A chatter's avatar as everyone sees it (ADR-0235): one of the shipped set (`avatars.ts`), or
 *  their own picture once staff approved it (`GET /api/v1/chat/avatars/{id}`). */
export type Avatar = { kind: 'stock'; id: string } | { kind: 'upload'; id: string };

export interface Author {
  id: string;
  name: string;
  standing: Standing;
  guest: boolean;
  /** Null or absent: the plain default, their initial. */
  avatar?: Avatar | null;
}

export interface Msg {
  id: number;
  kind: MessageKind;
  body: string;
  author: Author;
  reply_to: { id: number; name: string } | null;
  /** The server's clock when it arrived, epoch ms (ADR-0038). */
  server_ms: number;
  /** The programme time the sender was seeing, epoch ms; null when their player had none. */
  playhead_ms: number | null;
  state: MsgState;
  pinned: boolean;
  featured: boolean;
  /** [emoji, count] in the fixed order of REACTIONS. */
  reactions: [string, number][];
  /** A picture or document the message carries (ADR-0262). */
  attachment?: Attachment | null;
}

/** A picture or document posted in the chat (ADR-0262). The type is the one the station found in
 *  its bytes; the name is the chatter's and is shown as text only. */
export interface Attachment {
  id: string;
  kind: 'picture' | 'document';
  name: string;
  mime: string;
  /** The type in words: "PDF", "OpenDocument text", "PNG". */
  label: string;
  bytes: number;
  width?: number;
  height?: number;
  thumb_width?: number;
  thumb_height?: number;
}

/** Where a posted attachment is served: the picture, its thumbnail, or the document's download. */
export function attachmentUrl(a: Pick<Attachment, 'id' | 'kind'>, which: 'full' | 'thumb' = 'full'): string {
  if (a.kind === 'document') return `/api/v1/chat/files/${a.id}`;
  return which === 'thumb' ? `/api/v1/chat/pictures/${a.id}/thumb` : `/api/v1/chat/pictures/${a.id}`;
}

/** A file's size in words: "840 bytes", "12 KB", "2.4 MB". */
export function sizeWords(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The reactions a chatter can add (custom emoji come later). */
export const REACTIONS = ['👍', '❤️', '😂', '😮', '👏', '📻'] as const;

/** The room's modes and limits now. */
export interface RoomRules {
  slow: boolean;
  hold_new: boolean;
  links_trusted: boolean;
  accounts_only: boolean;
  min_age_24h: boolean;
  influx: boolean;
  /** Comments and reactions only: no replies between chatters (ADR-0035). */
  comments_only: boolean;
  /** Guests may post, every message held (ADR-0034). */
  guests: boolean;
  slow_s: number;
  min_age_minutes: number;
  max_chars: number;
  hold_ms: number;
  /** When chatters may post (ADR-0234). */
  hours: ChatHours;
  /** Chatters may upload a picture of their own now (ADR-0259), the studio's switch. */
  avatar_uploads?: boolean;
  /** Trusted chatters may attach a picture or document now (ADR-0262), the studio's switch. */
  attachments?: boolean;
}

/** The chat's hours (ADR-0234): always open, or during scheduled shows only. */
export interface ChatHours {
  /** The station opens the chat during shows only. */
  shows_only: boolean;
  /** Chatters may post now; the station's staff always may. */
  open: boolean;
  /** While closed: when the next scheduled show starts (epoch ms), or null when none is. */
  opens_at_ms: number | null;
  /** The station's time zone (IANA), to say that time in. */
  zone: string;
}

export interface Me {
  id: string;
  name: string;
  kind: AccountKind;
  standing: Standing;
  created_ms: number;
  timed_out_until_ms: number | null;
  banned: boolean;
  moderator: boolean;
  /** What everyone else sees beside their messages. */
  avatar?: Avatar | null;
  /** The shipped avatar they chose, shown while no picture of their own is approved. */
  avatar_stock?: string | null;
  /** Their own picture waiting for staff: only they see it (`GET /api/v1/chat/me/avatar`). */
  avatar_held?: string | null;
}

/** A statement of reasons (DSA Article 17). */
export interface Statement {
  id: string;
  at_ms: number;
  text: string;
  automated: boolean;
  review_by_ms: number;
  review_requested: boolean;
}

export interface Held {
  m: Msg;
  /** When it releases by itself; null: it waits for a moderator (the word filter flagged it). */
  release_at_ms: number | null;
  why: string;
  flagged: string | null;
  account_since_ms: number;
}

export type ItemKind = 'report' | 'notice' | 'review';

export interface AccountRef {
  id: string;
  name: string;
  since_ms: number;
  standing: Standing;
  guest: boolean;
  /** 'banned', 'timed out until 20:24', or null. */
  restricted: string | null;
  avatar?: Avatar | null;
}

/** One thing that needs a moderator: a report (one item per message, counted), a notice from
 *  someone without an account, or a chatter asking for a review of a decision. */
export interface Item {
  id: string;
  kind: ItemKind;
  reason: Reason;
  count: number;
  message: Msg | null;
  account: AccountRef | null;
  detail: string;
  location: string;
  contact: { name: string; email: string } | null;
  created_ms: number;
  /** An intimate image's 48-hour clock. */
  due_ms: number | null;
  /** The automated step involved ('rustrict'), named in the statement. */
  automated: string | null;
  status: 'open' | 'actioned' | 'dismissed' | 'acknowledged' | 'escalated';
  /** A report about the account's picture (ADR-0260): its id while it is still the one showing. */
  picture?: string | null;
  /** A report about a posted picture or document (ADR-0262): its id while it is still showing. */
  attachment?: string | null;
}

export interface StudioMsg {
  m: Msg;
  account_since_ms: number;
  passed_ms: number | null;
  reply: string | null;
  invited_ms: number | null;
}

/** An uploaded picture waiting for staff (ADR-0235); its file is at
 *  `GET /api/v1/chat/mod/avatars/{id}`. Pictures and documents posted in the chat wait in the
 *  same queue (ADR-0262): `kind` says which, with the attachment and its message; their files are
 *  at `attachmentUrl`, with a bearer. */
export interface HeldAvatar {
  id: string;
  account: AccountRef;
  uploaded_ms: number;
  bytes: number;
  /** Absent from a chat service older than 1.0.1, which queued avatars only. */
  kind?: 'avatar' | 'picture' | 'document';
  attachment?: Attachment;
  message?: { id: number; body: string };
}

export interface ModSnapshot {
  items: Item[];
  held: Held[];
  studio: StudioMsg[];
  /** Pictures waiting for approval, oldest first; empty for a moderator who is not staff. */
  avatars?: HeldAvatar[];
}

// ---- socket frames ----

export type ClientFrame =
  | { t: 'hello'; session?: string; since?: Position }
  | { t: 'post'; nonce: string; kind: Exclude<MessageKind, 'announcement'> | 'announcement'; body: string; reply_to?: number; playhead_ms?: number }
  | { t: 'react'; id: number; emoji: string }
  | { t: 'ping' };

export type AckState = 'visible' | 'held' | 'refused';

/** Refusal codes: banned, timed_out, accounts_only, guests_off, too_new, empty, too_long,
 *  replies_off, staff_only, closed (between shows, ADR-0234), links, too_fast, slow, repeat, run,
 *  signed_out, unknown_reply. */
export type RefusalCode = string;

export type ServerFrame =
  | {
      t: 'welcome';
      epoch: string;
      offset: number;
      recovered: boolean;
      missed: ServerFrame[];
      history: Msg[];
      station: string;
      show: string;
      me: Me | null;
      rules: RoomRules;
      /** How far behind the studio listeners hear (a setting until measured, ADR-0103). */
      listener_delay_ms: number;
      server_now_ms: number;
      pinned: Msg | null;
      count: number;
      statements: Statement[];
      moderation: ModSnapshot | null;
    }
  | { t: 'msg'; offset: number; m: Msg }
  | { t: 'hide'; offset: number; ids: number[] }
  | { t: 'pin'; offset: number; m: Msg | null }
  | { t: 'feature'; offset: number; m: Msg }
  | { t: 'react'; offset: number; id: number; reactions: [string, number][] }
  /** A chatter's avatar changed: every message of theirs on the page takes it. */
  | { t: 'avatar'; offset: number; account: string; avatar: Avatar | null }
  | { t: 'count'; n: number }
  | { t: 'rules'; rules: RoomRules; show: string; listener_delay_ms: number }
  | { t: 'ack'; nonce: string; id: number | null; state: AckState; code: RefusalCode | null; words: string | null; retry_ms: number | null }
  | { t: 'you'; me: Me }
  | { t: 'statement'; s: Statement }
  | { t: 'private'; id: string; at_ms: number; from: string; body: string; invite: boolean }
  | { t: 'mine'; m: Msg }
  | { t: 'released'; ids: number[] }
  /** Held messages a moderator turned down, to their sender (the statement follows). */
  | { t: 'rejected'; ids: number[] }
  | { t: 'mod_held'; h: Held }
  | { t: 'mod_unheld'; ids: number[] }
  | { t: 'mod_avatar'; a: HeldAvatar }
  | { t: 'mod_avatar_done'; id: string }
  | { t: 'mod_item'; item: Item }
  | { t: 'mod_item_done'; id: string; status: string; by: string }
  | { t: 'mod_studio'; s: StudioMsg }
  | { t: 'mod_done'; by: string; what: string; at_ms: number }
  | { t: 'pong' }
  | { t: 'error'; words: string };

/** Close codes: 4008 the client fell behind (reconnect and recover); 4001 signed out. */
export const CLOSE_SLOW = 4008;
export const CLOSE_SIGNED_OUT = 4001;

// ---- HTTP shapes ----

/** Report reasons, with the screener's keys in the statement dialog (A S T I M E) and the
 *  extra one only chatters and notices use. */
export type Reason = 'abuse' | 'spam' | 'off_topic' | 'illegal' | 'intimate_image' | 'threat_to_life' | 'child_safety' | 'other' | 'suspected_csam';

export interface ChatInfo {
  station: string;
  /** The station's terms, as it wrote them (may be empty: the page says so plainly). */
  terms: string;
  terms_version: string;
  accountable_person: string;
  contact: string;
  /** The minimum age a chatter declares (the station's setting). */
  min_age: number;
  rules: RoomRules;
  /** Each reason with its words, for the report and notice forms. */
  reasons: { reason: Reason; words: string; hint: string }[];
  /** What the chat's mode means, in a sentence (ADR-0035: the setup says it plainly). */
  mode_words: string;
  /** How long messages, reports and logs are kept, in a sentence each. */
  retention: { messages: string; moderation: string; addresses: string };
  /** Avatars (ADR-0235): whether a signed-in chatter may upload a picture, and its limits.
   *  Absent from a chat service older than 1.0.1. */
  avatars?: { uploads: boolean; max_upload_kb: number; side: number };
  /** Pictures and documents in the chat (ADR-0262): whether trusted chatters may attach now, the
   *  caps, and what the file chooser offers (the station checks the bytes, not this). */
  attachments?: { uploads: boolean; max_picture_kb: number; max_document_kb: number; accept: string };
}

export interface SignedIn {
  session: string;
  me: Me;
}

export interface ReportRequest {
  message_id?: number;
  account_id?: string;
  /** With `account_id`: report their picture, which must be the one showing (ADR-0260). */
  avatar_id?: string;
  /** With `message_id`: report the picture or document the message carries (ADR-0262). */
  attachment_id?: string;
  reason: Reason;
  detail?: string;
}

export interface NoticeRequest {
  reason: Reason;
  /** Where it is: a message, a time in a replay, a name. */
  location: string;
  /** Why it is unlawful or breaks the terms (DSA Art. 16(2)(a)). */
  detail: string;
  name: string;
  email: string;
  /** The notifier's statement that the notice is accurate and complete (Art. 16(2)(d)). */
  good_faith: boolean;
}

/** What a moderator or the studio does. Each is logged with `by`, the reason and the time; the
 *  ones that act on a chatter send them a statement of reasons. */
export type ModCommand =
  | { op: 'hide'; id: number; reason: Reason; note?: string; then?: 'none' | 'timeout' | 'ban'; automated_disclosed?: boolean; report?: string }
  | { op: 'release'; id: number }
  | { op: 'release_all' }
  | { op: 'hide_all_held'; reason: Reason }
  | { op: 'timeout'; account: string; minutes: number; reason: Reason; note?: string; report?: string }
  | { op: 'ban'; account: string; reason: Reason; note?: string; report?: string }
  | { op: 'lift'; account: string }
  | { op: 'dismiss'; report: string; note?: string }
  | { op: 'acknowledge'; report: string }
  | { op: 'escalate'; report: string; note?: string }
  | { op: 'release_hold'; hold: string }
  | { op: 'pin'; id: number }
  | { op: 'unpin' }
  | { op: 'feature'; id: number }
  | { op: 'post'; body: string; kind?: 'chat' | 'announcement'; pin?: boolean }
  | { op: 'standing'; account: string; standing: Exclude<Standing, 'staff'> }
  | { op: 'reply'; id: number; body: string }
  /** Staff only: everyone sees the waiting picture from now on. */
  | { op: 'avatar_approve'; avatar: string }
  /** Staff only: the waiting picture is deleted and its owner told why. */
  | { op: 'avatar_refuse'; avatar: string; reason: Reason; note?: string }
  /** Delete a kept picture at once, with why; one kept as evidence needs `confirm_evidence` (ADR-0257). */
  | { op: 'avatar_delete'; avatar: string; why: string; confirm_evidence?: boolean }
  /** An approved picture is taken down and its owner told why. */
  | { op: 'avatar_remove'; account: string; reason: Reason; note?: string; report?: string }
  /** A posted picture or document that was shown is taken down with its message, its author
   *  told why (ADR-0262). `avatar_approve` and `avatar_refuse` decide a waiting one. */
  | { op: 'attachment_remove'; attachment: string; reason: Reason; note?: string; report?: string }
  | { op: 'pass'; id: number }
  | { op: 'invite'; id: number };

export interface LogEntry {
  id: number;
  at_ms: number;
  actor: string;
  action: string;
  account: string | null;
  account_name: string | null;
  message_id: number | null;
  reason: string;
  note: string;
  automated: boolean;
  until_ms: number | null;
}

export interface StationChatSettings {
  terms: string;
  accountable_person: string;
  contact: string;
  min_age: number;
  /** ADR-0035: full chat (false) or comments and reactions only (true). */
  comments_only: boolean;
  guests: boolean;
  blocked_words: string[];
  forbidden_names: string[];
}

// ---- pure helpers both pages use ----

/** A refusal's wait, as the composer counts it down: "7 s", "6 min", "3 h". */
export function waitWords(ms: number): string {
  const s = Math.ceil(ms / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.ceil(s / 60)} min`;
  return `${Math.ceil(s / 3600)} h`;
}

/** Whether a viewer who syncs chat to their stream should see a message yet: the viewer hears
 *  the programme `delayMs` behind the studio, so a message written at studio time T shows once
 *  the viewer's clock (corrected by `skewMs`, server minus local) passes T + delay. */
export function dueForViewer(m: Pick<Msg, 'server_ms'>, nowLocalMs: number, skewMs: number, delayMs: number): boolean {
  return nowLocalMs + skewMs >= m.server_ms + delayMs;
}

/** Apply a public event to a list of room messages, as both pages keep them (newest last,
 *  at most `cap`). Returns the new list; unknown events leave it as it was. */
export function applyEvent(list: Msg[], f: ServerFrame, cap = 500): Msg[] {
  switch (f.t) {
    case 'msg': {
      if (list.some((m) => m.id === f.m.id)) return list.map((m) => (m.id === f.m.id ? f.m : m));
      const next = [...list, f.m];
      return next.length > cap ? next.slice(next.length - cap) : next;
    }
    case 'hide': {
      const gone = new Set(f.ids);
      return list.filter((m) => !gone.has(m.id));
    }
    case 'react':
      return list.map((m) => (m.id === f.id ? { ...m, reactions: f.reactions } : m));
    case 'feature':
      return list.map((m) => (m.id === f.m.id ? f.m : m));
    case 'pin':
      return list.map((m) => ({ ...m, pinned: f.m !== null && m.id === f.m.id }));
    case 'avatar':
      return list.some((m) => m.author.id === f.account) ? list.map((m) => (m.author.id === f.account ? withAvatar(m, f.avatar) : m)) : list;
    default:
      return list;
  }
}

/** A message with its author's avatar changed. */
export function withAvatar(m: Msg, avatar: Avatar | null): Msg {
  return { ...m, author: { ...m.author, avatar } };
}

/** Whether a message mentions a name: `@name`, any case, not inside a longer word. */
export function mentions(body: string, name: string): boolean {
  if (!name) return false;
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`(^|[^\\p{L}\\p{N}_])@${esc}(?![\\p{L}\\p{N}_])`, 'iu').test(body);
}
