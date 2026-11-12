// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The public pages' link to the control plane: the player's facts, call-in, and the routes a
// person's own pass opens (a fresh room token, their own state, hanging up). No admin bearer
// ever reaches these pages; a pass is the only credential, and it names one person.

import type { StreamChoice } from './streams.ts';

export type Role = 'presenter' | 'dj' | 'guest' | 'caller' | 'screener' | 'producer' | 'listener';

export interface PlayerInfo {
  station: string;
  show: string;
  stream_url: string;
  /** Every stream the listener may choose (ADR-0232): Opus first, then each MP3 stream. */
  streams: StreamChoice[];
  presenter: string;
  presenter_live: boolean;
  show_on_air: boolean;
  call_in: {
    open: boolean;
    why_not: string | null;
    consent_text: string;
    consent_version: string;
    name_max: number;
    topic_max: number;
  };
  relay: { turn_tls_port: number };
  /** The desk's HLS while the station has video on (ADR-0009); null otherwise. */
  video: { live: string; audio: string } | null;
}

/** What joining returns: a room token, the pass that renews it, and the one track to hear. */
export interface Joined {
  pass: string;
  token: string;
  url: string;
  room: string;
  identity: string;
  role: Role;
  name: string;
  /** The desk's participant carrying this person's mix-minus: the only one to subscribe to. */
  hear: string;
  /** While the desk has video on: its programme picture's participant, the only picture to
   * watch (ADR-0028); the page then offers the camera. */
  see?: string | null;
  pass_expires_at_ms: number;
}

export interface PassInfo {
  kind: 'call' | 'invite';
  role: Role;
  identity: string;
  name: string;
  station: string;
  show: string;
  expires_at_ms: number;
  ended: boolean;
  relay: { turn_tls_port: number };
  /** Why the pass cannot join the room now, in the station's sentence, while the show on air
   *  runs at a step that has switched its seat off (ADR-0271); the pass stays good for later.
   *  Null or absent when it can join. */
  not_now?: string | null;
}

export type Phase = 'joining' | 'waiting' | 'screening' | 'ready' | 'desk_busy' | 'cue' | 'on_air' | 'off_air' | 'ended';

/** One person's own state, as the control plane computes it (crates/control/src/join.rs). */
export interface MeState {
  identity: string;
  role: Role;
  phase: Phase;
  open: boolean;
  muted: boolean;
  place: number | null;
  queue: number;
  screener_talking: boolean;
  talkback: boolean;
  held_until_ms: number | null;
  station: string;
  show: string;
  presenter: string;
  presenter_live: boolean;
  show_on_air: boolean;
  desk_reachable: boolean;
  /** Why a waiting caller was let go when the lines closed under them (ADR-0271): a show that
   *  takes no calls took the air, or the step of the show on air was moved down. Null or absent
   *  otherwise. */
  released?: Released | null;
  /** The presenter answers the phones themselves (the Callers step has no screener): name them, not a producer. */
  presenter_answers?: boolean;
}

/** Why the lines closed under a waiting caller. */
export type Released = 'show_took_air' | 'step_moved_down';

/** A refusal from the control plane: its status, its sentence, and the field it is about. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly field: string | null = null,
  ) {
    super(message);
  }
}

async function json<T>(res: Response): Promise<T> {
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`, data.field ?? null);
  return data as T;
}

const bearer = (pass: string) => ({ Authorization: `Bearer ${pass}` });

export const player = () => fetch('/api/v1/player').then((r) => json<PlayerInfo>(r));

export interface CallInRequest {
  name: string;
  topic: string;
  consent: { accepted: boolean; version: string } | null;
  /** The answered challenge (lib/challenge.ts); left out where the station turned it off. */
  challenge?: ChallengeAnswer;
}

/** Call-in's challenge (crates/control/src/challenge.rs). */
export interface Challenge {
  token: string;
  salt: string;
  bits: number;
  expires_at_ms: number;
}

export interface ChallengeAnswer {
  token: string;
  nonce: string;
}

export const challenge = () => fetch('/api/v1/call-in/challenge', { cache: 'no-store' }).then((r) => json<Challenge>(r));

export const callIn = (req: CallInRequest) =>
  fetch('/api/v1/call-in', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(req) }).then((r) =>
    json<Joined>(r),
  );

export const passInfo = (pass: string) => fetch('/api/v1/pass', { headers: bearer(pass) }).then((r) => json<PassInfo>(r));

export const passToken = (pass: string) => fetch('/api/v1/pass/token', { method: 'POST', headers: bearer(pass) }).then((r) => json<Joined>(r));

export const leave = (pass: string) => fetch('/api/v1/pass/leave', { method: 'POST', headers: bearer(pass) }).then(() => undefined);

/** Follow one's own state; reconnects after a second while the page lives. Returns a stop. */
export function followMe(pass: string, onState: (s: MeState) => void, onGone: (why: string) => void): () => void {
  let stopped = false;
  let ctrl: AbortController | null = null;
  const run = async () => {
    while (!stopped) {
      ctrl = new AbortController();
      try {
        const res = await fetch('/api/v1/pass/events', { headers: bearer(pass), signal: ctrl.signal });
        if (res.status === 401 || res.status === 410) {
          const data = await res.json().catch(() => ({}));
          onGone(data.error ?? 'This link no longer works.');
          return;
        }
        if (!res.ok || !res.body) throw new Error(`${res.status}`);
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
        let buf = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += value;
          let cut: number;
          while ((cut = buf.indexOf('\n\n')) >= 0) {
            const frame = buf.slice(0, cut);
            buf = buf.slice(cut + 2);
            const data = frame
              .split('\n')
              .filter((l) => l.startsWith('data:'))
              .map((l) => l.slice(5).trimStart())
              .join('\n');
            if (data) onState(JSON.parse(data) as MeState);
          }
        }
      } catch {
        if (stopped) return;
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
  };
  void run();
  return () => {
    stopped = true;
    ctrl?.abort();
  };
}
