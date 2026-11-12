// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The chat's HTTP routes, as the listener's page calls them (packages/ui/chat.ts has the list).
// Every failure becomes a sentence a listener can act on: the server's own `error` when it sent
// one, otherwise a plain line, never a raw status. The session token is kept on this device;
// browser storage can be refused, so every access is guarded and the chat works without it.

import type { Attachment, ChatInfo, Me, NoticeRequest, Reason, ReportRequest, SignedIn, Statement } from '../../../../../packages/ui/chat.ts';
import { ApiError } from '../api.ts';

export { ApiError };

const BASE = '/api/v1/chat';
const UNREACHABLE = "The chat couldn't be reached. Check your connection and try again.";
const SERVER = 'Something went wrong at the station. Try again in a moment.';
const AWAY = "The chat isn't available just now. Try again in a moment.";

/** What a 422 at sign-in still needs: `needs` when the server lists it, else the one field. */
export class NeedsError extends ApiError {
  constructor(
    status: number,
    message: string,
    field: string | null,
    readonly needs: string[],
  ) {
    super(status, message, field);
  }
}

async function call<T>(path: string, init: RequestInit = {}, session?: string | null): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.body) headers['Content-Type'] = 'application/json';
  if (session) headers.Authorization = `Bearer ${session}`;
  let res: Response;
  try {
    res = await fetch(`${BASE}${path}`, { ...init, headers });
  } catch {
    throw new ApiError(0, UNREACHABLE);
  }
  const data = (await res.json().catch(() => ({}))) as { error?: unknown; field?: unknown; needs?: unknown };
  if (!res.ok) {
    const away = res.status === 404 || res.status === 502 || res.status === 503 || res.status === 504;
    const words = typeof data.error === 'string' && data.error ? data.error : away ? AWAY : SERVER;
    const field = typeof data.field === 'string' ? data.field : null;
    if (res.status === 422 && field) {
      const needs = Array.isArray(data.needs) ? data.needs.filter((n): n is string => typeof n === 'string') : [field];
      throw new NeedsError(res.status, words, field, needs);
    }
    throw new ApiError(res.status, words, field);
  }
  return data as T;
}

const post = (body: unknown): RequestInit => ({ method: 'POST', body: JSON.stringify(body) });

export const info = () => call<ChatInfo>('/info');

export const signinStart = (email: string) => call<{ sent: boolean; expires_s: number }>('/signin/start', post({ email }));

export interface FinishRequest {
  email: string;
  code: string;
  name?: string;
  age_ok?: boolean;
  terms_version?: string;
}
export const signinFinish = (req: FinishRequest) => call<SignedIn>('/signin/finish', post(req));

export const guest = (req: { name: string; age_ok: boolean; terms_version: string }) => call<SignedIn>('/guest', post(req));

export const notice = (req: NoticeRequest) => call<{ id: string }>('/notices', post(req));

export const me = (session: string) => call<{ me: Me; statements: Statement[] }>('/me', {}, session);

export const signout = (session: string) =>
  fetch(`${BASE}/signout`, { method: 'POST', headers: { Authorization: `Bearer ${session}` } }).then(
    () => undefined,
    () => undefined,
  );

export const report = (session: string, req: ReportRequest) => call<{ id: string }>('/reports', post(req), session);

export const review = (session: string, id: string, text: string) => call<{ id: string }>(`/statements/${encodeURIComponent(id)}/review`, post({ text }), session);

// ---- avatars (ADR-0235) ----

/** Choose one of the shipped avatars, or none (null: the chatter's initial). */
export const chooseStock = (session: string, id: string | null) => call<{ me: Me }>('/me/avatar/stock', { method: 'PUT', body: JSON.stringify({ id }) }, session);

/** Take the chatter's own picture away, waiting or approved. */
export const removeAvatar = (session: string) => call<{ me: Me }>('/me/avatar', { method: 'DELETE' }, session);

/** Send the chatter's own picture, already cropped square. It waits for a moderator. */
export async function uploadAvatar(session: string, picture: Blob): Promise<{ me: Me }> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/me/avatar`, { method: 'PUT', headers: { Authorization: `Bearer ${session}`, 'Content-Type': picture.type || 'image/jpeg' }, body: picture });
  } catch {
    throw new ApiError(0, UNREACHABLE);
  }
  const data = (await res.json().catch(() => ({}))) as { error?: unknown; me?: Me };
  if (!res.ok || !data.me) {
    const words =
      typeof data.error === 'string' && data.error
        ? data.error
        : res.status === 413
          ? 'That picture is too large to send. Choose a smaller one.'
          : res.status === 404 || res.status >= 502
            ? AWAY
            : SERVER;
    throw new ApiError(res.status, words);
  }
  return { me: data.me };
}

/** The chatter's own picture as only they may fetch it (the one waiting, else the approved
 *  one): an object URL to show, which the caller revokes; null when there is none. */
export async function ownAvatar(session: string): Promise<string | null> {
  try {
    const res = await fetch(`${BASE}/me/avatar`, { headers: { Authorization: `Bearer ${session}` }, cache: 'no-store' });
    if (!res.ok) return null;
    return URL.createObjectURL(await res.blob());
  } catch {
    return null;
  }
}

// ---- pictures and documents in the chat (ADR-0262) ----

export interface AttachRequest {
  nonce: string;
  /** The file's own name, shown as text beside it. */
  name: string;
  body: string;
  reply_to?: number;
  playhead_ms?: number;
}

/** Post a message with a picture or document: the file is the body, the rest in the query. It
 *  waits for the station; the answer is the message's id and the attachment as the room will
 *  show it. */
export async function attach(session: string, file: Blob, req: AttachRequest): Promise<{ id: number; attachment: Attachment }> {
  const q = new URLSearchParams({ nonce: req.nonce, name: req.name, body: req.body });
  if (req.reply_to !== undefined) q.set('reply_to', String(req.reply_to));
  if (req.playhead_ms !== undefined) q.set('playhead_ms', String(req.playhead_ms));
  let res: Response;
  try {
    res = await fetch(`${BASE}/attachments?${q}`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${session}`, 'Content-Type': 'application/octet-stream' },
      body: file,
    });
  } catch {
    throw new ApiError(0, UNREACHABLE);
  }
  const data = (await res.json().catch(() => ({}))) as { error?: unknown; id?: number; attachment?: Attachment };
  if (!res.ok || typeof data.id !== 'number' || !data.attachment) {
    const words =
      typeof data.error === 'string' && data.error
        ? data.error
        : res.status === 413
          ? 'That file is too large to send.'
          : res.status === 404 || res.status >= 502
            ? AWAY
            : SERVER;
    throw new ApiError(res.status, words);
  }
  return { id: data.id, attachment: data.attachment };
}

export type { Reason };

// ---- kept on this device ----

const SESSION = 'weavesignal.chat.session';
const PREFS = 'weavesignal.chat.prefs';

export function loadSession(): string | null {
  try {
    return localStorage.getItem(SESSION);
  } catch {
    return null;
  }
}

export function saveSession(token: string | null): void {
  try {
    if (token) localStorage.setItem(SESSION, token);
    else localStorage.removeItem(SESSION);
  } catch {
    // Refused: the session lasts as long as the page.
  }
}

export function loadPrefs(): string | null {
  try {
    return localStorage.getItem(PREFS);
  } catch {
    return null;
  }
}

export function savePrefs(raw: string): void {
  try {
    localStorage.setItem(PREFS, raw);
  } catch {
    // Refused: the settings last as long as the page.
  }
}
