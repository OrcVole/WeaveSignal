// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A pulled source (1.2, PLAN 17b): a stream the station opens itself, from an address kept with
// the show's key, while the show's door is open. It then stands by on the desk like any host's
// own studio. The station checks the address when it is saved (crates/proto/src/pull.rs and
// `check_pull_url`); the checks here are the same ones that need no network, so a slip is said
// under the field as it is typed, and the station's own refusals are put into plainer words.

import type { SourceNow } from './api.ts';
import { clockWords } from './hosts.ts';

export type PullKind = 'http' | 'hls' | 'srt';

export const PULL_KIND_WORD: Record<PullKind, string> = {
  http: 'Icecast or other audio over the web',
  hls: 'HLS (a playlist)',
  srt: 'SRT: the station calls it',
};

export const PULL_URL_MAX = 2048;

/** What kind of stream an address is, as the station will read it; null while it is not one. */
export function pullKindOf(raw: string): PullKind | null {
  const u = raw.trim();
  const m = /^(https?|srt):\/\/[^/?#\s]+/i.exec(u);
  if (!m) return null;
  if (m[1].toLowerCase() === 'srt') return 'srt';
  const path = u.slice(m[0].length).split(/[?#]/)[0];
  return path.toLowerCase().endsWith('.m3u8') ? 'hls' : 'http';
}

/** What is wrong with an address, before the station is asked; null when it may be sent. */
export function pullProblem(raw: string): string | null {
  const u = raw.trim();
  if (!u) return 'Give the stream’s address, for example https://radio.example.org/live.mp3.';
  if (u.length > PULL_URL_MAX) return 'An address is at most 2,048 characters.';
  if (/\s/.test(u)) return 'An address has no spaces.';
  if (!/^(https?|srt):\/\//i.test(u)) return 'Start the address with http://, https:// or srt://.';
  const rest = u.replace(/^[a-z]+:\/\//i, '');
  const authority = rest.split(/[/?#]/)[0];
  if (!authority) return 'The address has no server name.';
  if (authority.includes('@')) return 'Leave out the user name and password: the station cannot keep them. Use a stream that needs none.';
  if (/^srt:/i.test(u)) {
    if (!/:\d{1,5}$/.test(authority)) return 'An srt:// address needs a port, for example srt://studio.example.org:9000.';
    if (/[?&]passphrase=/i.test(u)) return PASSPHRASE;
  }
  return null;
}

const PASSPHRASE = 'The station cannot keep an SRT passphrase yet. Use an address without one, or have the host send to the station with a key instead.';

/** The station's refusal of an address, in plainer words where its sentence is a known one;
 *  any other sentence is shown as the station gave it. */
export function pullRefusal(message: string): string {
  const m = message.trim();
  if (/inside the station's own network/i.test(m)) return 'That address is inside the station’s own network, which the station does not open. Use the stream’s public address.';
  if (/passphrase cannot be kept/i.test(m)) return PASSPHRASE;
  if (/does not resolve/i.test(m)) return `${m.charAt(0).toUpperCase()}${m.slice(1).replace(/`/g, '')}.`;
  if (/must be a listener/i.test(m)) return 'The station calls an SRT address, so the other end must be listening. Leave mode out of the address, or set mode=caller.';
  if (/user name or password/i.test(m)) return 'Leave out the user name and password: the station cannot keep them. Use a stream that needs none.';
  return m ? `${m.charAt(0).toUpperCase()}${m.slice(1)}${/[.!?]$/.test(m) ? '' : '.'}` : 'The station refused that address.';
}

export interface PullStatus {
  /** `on_air`, `standby`, `test`, `trying` (the door is open and nothing has arrived), `closed`
   *  (outside the show's door: the station is not trying). */
  state: 'on_air' | 'standby' | 'test' | 'trying' | 'closed';
  /** One line on where it stands. */
  words: string;
}

/** Where a pull stands, from what the station lists of its key now. */
export function pullStatus(now: SourceNow | null | undefined, zone: string): PullStatus {
  if (!now) return { state: 'closed', words: 'Not open now. The station opens it before the show’s time, and it stands by until then.' };
  const fmt = now.format ? ` · ${now.format}` : '';
  if (now.state === 'on_air') return { state: 'on_air', words: `On air${fmt}` };
  if (now.state === 'standby') return { state: 'standby', words: `Open and standing by on the desk${fmt}` };
  if (now.state === 'test') return { state: 'test', words: `Open as a private test${fmt}` };
  const until = now.door_closes_ms != null ? ` It keeps trying until ${clockWords(now.door_closes_ms, zone)}.` : '';
  return { state: 'trying', words: `Nothing arrives from the address yet.${until}` };
}
