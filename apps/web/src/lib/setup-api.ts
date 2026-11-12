// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The setup page's link to the control plane (ADR-0237). Every route here needs an admin: the
// station key, or an admin's staff session. The bearer lives in the page's memory only.
//
// Behind Cloudron's login a refusal does not arrive as 401: Cloudron turns it into a redirect
// to its own /login. Every request is therefore sent with redirects off, and a redirect is
// read as the refusal it was, so a wrong key can never look like a page that loaded.
import type { Look } from './brand.svelte.ts';

export interface Answer {
  value: string;
  source: 'page' | 'settings';
  settings_value: string;
}
export interface SetupView {
  status: 'pending' | 'done';
  done_by: 'page' | 'settings' | null;
  missing: string[];
  completed_at_ms: number | null;
  name: Answer;
  contact: Answer;
  stream_password: { available: boolean; enabled: boolean | null; asked: boolean | null; restart_needed: boolean };
  install: {
    public_url: string;
    stream_url: string;
    room_url: string;
    media_port: number;
    turn_tls_port: number;
    mail: 'command' | 'outbox' | 'off';
    listed: boolean;
  };
  branding: Look;
  updated_by: string | null;
  updated_at_ms: number | null;
}
export interface Preset {
  key: string;
  name: string;
  summary: string;
  licensed: boolean;
}
export interface RetentionView {
  setting: { preset: string };
  presets: Preset[];
}

/** A refusal or a failure, with a sentence for the person and the field it is about, if any. */
export class Problem extends Error {
  constructor(
    public status: number,
    message: string,
    public field?: string,
  ) {
    super(message);
  }
  /** The sign-in is not accepted (or has ended): ask again. */
  get signIn(): boolean {
    return this.status === 401;
  }
}

const NOT_REACHED = 'The station did not answer. Check your connection and try again.';

async function send(method: string, path: string, bearer: string, body?: BodyInit, type?: string): Promise<Response> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      // Behind Cloudron's login a request passes only with a bearer; signing in sends a
      // placeholder, as the studio app does.
      headers: { Authorization: `Bearer ${bearer || 'sign-in'}`, Accept: 'application/json', ...(type ? { 'Content-Type': type } : {}) },
      body,
      redirect: 'manual',
      cache: 'no-store',
      credentials: 'same-origin',
    });
  } catch {
    throw new Problem(0, NOT_REACHED);
  }
  if (res.type === 'opaqueredirect' || (res.status >= 300 && res.status < 400)) {
    throw new Problem(401, 'The station did not accept that. Check it and try again.');
  }
  if (!res.ok) {
    let message = '';
    let field: string | undefined;
    try {
      const j = (await res.json()) as { error?: string; field?: string };
      message = j.error ?? '';
      field = j.field;
    } catch {
      // not JSON: a proxy's own page
    }
    if (res.status === 401) message = 'The station did not accept that. Check it and try again.';
    else if (res.status === 403) message = 'Only an admin can set the station up. Ask the station’s admin, or use the station key.';
    else if (res.status === 413) message = 'That file is too large.';
    else if (res.status === 429) message = 'Too many tries just now. Wait a few minutes and try again.';
    else if (!message) message = res.status >= 500 ? 'The station could not do that just now. Try again in a minute.' : 'The station refused that.';
    throw new Problem(res.status, message, field);
  }
  return res;
}

async function json<T>(method: string, path: string, bearer: string, body?: unknown): Promise<T> {
  const res = await send(method, path, bearer, body === undefined ? undefined : JSON.stringify(body), body === undefined ? undefined : 'application/json');
  return (await res.json()) as T;
}

/** Sign in as a member of staff: the session's token. */
export async function signIn(username: string, password: string): Promise<string> {
  const res = await json<{ token: string }>('POST', '/api/v1/sessions', '', { username, password });
  return res.token;
}
export const signOut = (bearer: string) => send('DELETE', '/api/v1/sessions/me', bearer).then(() => undefined);

export const view = (bearer: string) => json<SetupView>('GET', '/api/v1/setup', bearer);
export const save = (bearer: string, name: string, contact: string) => json<SetupView>('PUT', '/api/v1/setup', bearer, { name, contact });
export const finish = (bearer: string) => json<SetupView>('POST', '/api/v1/setup/finish', bearer);
export const streamPassword = (bearer: string, enabled: boolean, password: string) =>
  json<SetupView>('PUT', '/api/v1/setup/stream-password', bearer, password ? { enabled, password } : { enabled });

export const retention = (bearer: string) => json<RetentionView>('GET', '/api/v1/retention', bearer);
/** Choose a preset. `by` names who, when the bearer is the station key (a session names itself). */
export const choosePreset = (bearer: string, preset: string) => json<RetentionView>('PUT', '/api/v1/retention', bearer, { preset, by: 'the setup page' });

export const setAccent = (bearer: string, accent: string) => json<Look>('PUT', '/api/v1/branding/accent', bearer, { accent });
export async function setPicture(bearer: string, kind: 'icon' | 'banner', file: Blob): Promise<Look> {
  const res = await send('PUT', `/api/v1/branding/${kind}`, bearer, file, 'application/octet-stream');
  return (await res.json()) as Look;
}
export const removePicture = (bearer: string, kind: 'icon' | 'banner') => json<Look>('DELETE', `/api/v1/branding/${kind}`, bearer);

/** What each picture may be, checked here for a quick answer; the station checks again. */
export const PICTURE_RULES = {
  icon: { maxBytes: 1024 * 1024, hint: 'PNG or JPEG, up to 1 MB, at least 64 pixels on each side. A square works best. Transparency is kept.' },
  banner: { maxBytes: 4 * 1024 * 1024, hint: 'PNG or JPEG, up to 4 MB, at least 600 by 200 pixels. It is shown three times as wide as it is tall; a picture of another shape is cropped around its middle.' },
} as const;

/** A sentence if this file cannot be the picture, before it is sent. */
export function pictureProblem(kind: 'icon' | 'banner', file: { size: number; type: string; name: string }): string | null {
  const svg = file.type === 'image/svg+xml' || /\.svgz?$/i.test(file.name);
  if (svg) return 'SVG files are not taken. Export it as a PNG and choose that.';
  if (file.type && file.type !== 'image/png' && file.type !== 'image/jpeg') return 'Choose a PNG or a JPEG.';
  if (file.size > PICTURE_RULES[kind].maxBytes) return `That file is larger than ${PICTURE_RULES[kind].maxBytes / 1024 / 1024} MB. Choose a smaller one.`;
  if (file.size === 0) return 'That file is empty.';
  return null;
}
