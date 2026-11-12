// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The programme sent to another Icecast server (ADR-0226): the control plane's view of it, the
// choice an admin makes, and the words the Settings panel shows. The server's address and
// password are the deployment's settings; nothing here can read or change them.

import { ApiError, base, headers, netFetch } from './api.ts';

export type Mode = 'auto' | 'on' | 'off';

/** `GET /api/v1/icecast-out`, field for field. */
export interface IcecastOut {
  configured: boolean;
  mode: Mode;
  wanted: boolean;
  reason: string;
  connected: boolean;
  host: string;
  port: number;
  mount: string;
  tls: boolean;
  bitrate: number;
  bytes_sent: number;
  connected_since_ms: number | null;
  failures: number;
  last_error: string;
  last_error_at_ms: number | null;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${base()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as T;
}

export const icecastOut = () => call<IcecastOut>('GET', '/api/v1/icecast-out');
export const chooseMode = (mode: Mode) => call<IcecastOut>('PUT', '/api/v1/icecast-out', { mode });

/** The three choices, in the order shown. */
export const MODES: readonly { mode: Mode; label: string; help: string }[] = [
  {
    mode: 'auto',
    label: 'While the desk is on air',
    help: 'Sends while the station is live and the desk is on air. When you stop, the other station goes back to its own automation.',
  },
  {
    mode: 'on',
    label: 'Always',
    help: 'Sends whatever listeners hear, automation and fallback included, until you turn it off. Use it to test the connection.',
  },
  { mode: 'off', label: 'Never', help: 'Sends nothing.' },
];

/** Bytes as people read them. */
export function sizeWords(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Where it sends: `radio.example.org:8005/`, with `tls` when encrypted. */
export function targetWords(s: Pick<IcecastOut, 'host' | 'port' | 'mount' | 'tls'>): string {
  const mount = s.mount.startsWith('/') ? s.mount : `/${s.mount}`;
  return `${s.tls ? 'https://' : ''}${s.host}:${s.port}${mount}`;
}

export type Tone = 'good' | 'warning' | 'serious' | 'plain';

/** One line for what is happening, and how loud to say it. */
export function statusWords(s: IcecastOut): { tone: Tone; text: string } {
  if (!s.configured) return { tone: 'plain', text: 'Not set up on this station.' };
  const to = targetWords(s);
  if (s.connected) return { tone: 'good', text: `Sending to ${to}.` };
  if (s.wanted && s.failures > 0) return { tone: 'serious', text: `Cannot reach ${to}. Trying again.` };
  if (s.wanted) return { tone: 'warning', text: `Connecting to ${to}…` };
  return { tone: 'plain', text: `Not sending (${s.reason}).` };
}

/** The last error, only while it matters: not while connected. */
export function errorWords(s: IcecastOut): string {
  return !s.connected && s.last_error ? s.last_error : '';
}
