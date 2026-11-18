// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor link's routes (ADR-0280; crates/control/src/monitor.rs and monitor_view.rs): a
// host's own view of the station, as events, a listen-only token for their monitor room, and
// the six things they may press. The only credential is the link's secret, sent as a bearer and
// never in an address. Every sentence the station sends (`says`, `then.words`, `why_not`,
// `end_show_says`, a refusal's `error`) is printed as it is: the station owns the words.

import { ApiError } from '../api.ts';

/** One button: whether it may be pressed now, and if not, why, in the station's words. */
export interface Control {
  may: boolean;
  why_not: string | null;
}

export interface Controls {
  go: Control;
  not_yet: Control;
  cue: Control;
  bring: Control;
  hand_over: Control;
  end_show: Control;
  /** What follows End show, for the question. */
  end_show_says: string | null;
  dump: Control;
}

/** What happens next, in one sentence, and the instant it counts to on the station's clock. */
export interface Then {
  kind: string;
  words: string;
  at_ms: number | null;
}

export interface Post {
  show_name: string;
  start_ms: number;
  started: boolean;
  mine: boolean;
  host: string | null;
  host_state: string | null;
  on_air_at_ms: number | null;
  late_ends_ms: number | null;
  grace_ends_ms: number | null;
  held_by: string | null;
  on_air: string[];
}

export interface RundownRow {
  show_name: string;
  host: string | null;
  state: string;
  start_ms: number;
  end_ms: number;
}

export interface Insert {
  label: string;
  at_ms: number;
  duration_ms: number;
  /** Whether it plays over the host or instead of them: `unknown` today. */
  plays: string;
}

export interface Refusal {
  at_ms: number;
  reason: string;
  words: string;
}

export interface MonitorView {
  label: string;
  show: string;
  show_name: string;
  /** `on_air`, `open`, `standby`, `link_lost`, `test` or `not_connected`. */
  state: string;
  says: string;
  then: Then | null;
  way: string | null;
  format: string | null;
  /** LUFS after the station's leveller at the desk; in a test, the station's own dBFS reading. */
  level: number | null;
  leveller_db: number | null;
  steady_ms: number | null;
  title: string | null;
  monitor_open: boolean;
  door_closes_ms: number | null;
  last_refusal: Refusal | null;
  post: Post | null;
  rundown: { before: RundownRow | null; you: RundownRow | null; after: RundownRow | null };
  insert: Insert | null;
  step: { step: string; word: string; dump: boolean; handover: boolean };
  next_host: { name: string; state: string; cued: boolean } | null;
  listeners_behind_ms: number | null;
  /** The host's monitor mix carries what listeners hear (someone else is on air at the desk, and
   *  listeners hear the desk). False while automation holds the air: the mix is then silent, and
   *  a host standing by hears the listeners' stream instead. */
  monitor_has_programme: boolean;
  controls: Controls;
  station: { now_ms: number; zone: string };
}

/** What a press answers: what it did, and the view after it. */
export interface Done {
  done: string;
  view: MonitorView;
  passed_to: string | null;
  automation_in_ms: number | null;
  /** Dump: how much of the programme listeners lost. */
  lost_ms: number | null;
}

export interface RoomToken {
  token: string;
  url: string;
  room: string;
  identity: string;
  ttl_s: number;
}

/** The six presses, by their route. */
export type Press = 'go' | 'not-yet' | 'cue' | 'handover' | 'end-show' | 'dump';

const bearer = (secret: string) => ({ Authorization: `Bearer ${secret}` });

/** An answer, or an `ApiError` with the station's own sentence; its message is empty when the
 *  station (or a proxy in front of it) refused without one. */
async function json<T>(res: Response): Promise<T> {
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, typeof data.error === 'string' ? data.error : '');
  return data as T;
}

/** How long a press or a token may take. A press on a connection that has stalled must not
 *  hold the controls for ever: after this the page says the station did not answer. */
export const ANSWER_WITHIN_MS = 8000;

/** One press. A refusal is an `ApiError` with the station's sentence (409), 401 or 410 when the
 *  link is not good; a press that never reached the station, or was not answered in time,
 *  throws whatever `fetch` threw. */
export function press(secret: string, route: Press, body?: Record<string, unknown>): Promise<Done> {
  return fetch(`/api/v1/monitor/${route}`, {
    method: 'POST',
    headers: body ? { ...bearer(secret), 'Content-Type': 'application/json' } : bearer(secret),
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(ANSWER_WITHIN_MS),
  }).then((r) => json<Done>(r));
}

/** A fresh listen-only token for the monitor room (good for ten minutes to join with). */
export const roomToken = (secret: string) =>
  fetch('/api/v1/monitor/token', { method: 'POST', headers: bearer(secret), signal: AbortSignal.timeout(ANSWER_WITHIN_MS) }).then((r) => json<RoomToken>(r));

/** One server-sent event. */
export interface Frame {
  event: string;
  data: string;
}

/** Cut whole events off the front of what has arrived; the rest waits for more. A frame with no
 *  `event:` line (the keep-alive comment) is dropped. */
export function frames(buf: string): { frames: Frame[]; rest: string } {
  const out: Frame[] = [];
  let rest = buf.replaceAll('\r\n', '\n');
  let cut: number;
  while ((cut = rest.indexOf('\n\n')) >= 0) {
    const lines = rest.slice(0, cut).split('\n');
    rest = rest.slice(cut + 2);
    const field = (name: string) =>
      lines
        .filter((l) => l.startsWith(`${name}:`))
        .map((l) => l.slice(name.length + 1).replace(/^ /, ''))
        .join('\n');
    const event = field('event');
    if (event) out.push({ event, data: field('data') });
  }
  return { frames: out, rest };
}

/** How one attempt at the event stream ended. */
export type StreamEnd =
  /** The link is not good: its key has ended (410, or the stream said so) or the secret is wrong (401). */
  | { kind: 'gone'; words: string }
  /** Refused for now, with the station's sentence: too many pages on the link, or too many
   *  wrong links from this address (429). */
  | { kind: 'refused'; words: string; retry_ms: number }
  /** The station answered and cannot serve the page just now (5xx). */
  | { kind: 'unready' }
  /** The connection failed or closed. */
  | { kind: 'dropped' };

const GONE = 'This link no longer works. Ask the station for a new one.';

/**
 * Read the event stream once, until it ends: each `view` goes to `onView`. `fetch` and not
 * `EventSource`, because the secret rides in a header. The caller reconnects, and watches for a
 * stream that has gone quiet without closing.
 */
export async function readEvents(secret: string, signal: AbortSignal, onView: (v: MonitorView) => void): Promise<StreamEnd> {
  let res: Response;
  try {
    res = await fetch('/api/v1/monitor/events', { headers: { ...bearer(secret), Accept: 'text/event-stream' }, cache: 'no-store', signal });
  } catch {
    return { kind: 'dropped' };
  }
  if (!res.ok || !res.body) {
    const data = await res.json().catch(() => ({}));
    const words = typeof data.error === 'string' ? data.error : '';
    if (res.status === 401 || res.status === 410) return { kind: 'gone', words: words || GONE };
    if (res.status === 429) {
      const wait = Number(res.headers.get('Retry-After'));
      return { kind: 'refused', words: words || 'The station is refusing this page for now.', retry_ms: Number.isFinite(wait) && wait > 0 ? wait * 1000 : 10_000 };
    }
    return res.status >= 500 ? { kind: 'unready' } : { kind: 'dropped' };
  }
  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  let buf = '';
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return { kind: 'dropped' };
      const cut = frames(buf + value);
      buf = cut.rest;
      for (const f of cut.frames) {
        if (f.event === 'ended') {
          let words = GONE;
          try {
            const e = JSON.parse(f.data).error;
            if (typeof e === 'string' && e) words = e;
          } catch {
            // the station's own sentence, or ours
          }
          return { kind: 'gone', words };
        }
        if (f.event !== 'view') continue;
        try {
          onView(JSON.parse(f.data) as MonitorView);
        } catch {
          // a frame cut short: the next one is a second away
        }
      }
    }
  } catch {
    return { kind: 'dropped' };
  }
}
