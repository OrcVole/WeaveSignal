// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// RTMP out (ADR-0230; rtmp-out): the words Settings > RTMP out shows for each destination, and
// what its form checks before it asks the station (which checks again and has the last word).
import type { RtmpDestinationInfo } from './api.ts';

export const PROFILES = [
  { profile: 'compatible', name: 'H.264 and AAC', what: 'Classic RTMP: what Rumble, Odysee and PeerTube take.' },
  { profile: 'open', name: 'AV1 and Opus', what: 'Enhanced RTMP: for a server that takes it (MediaMTX does; Rumble and Odysee did not, 2026-11-06).' },
] as const;

export function profileName(p: string): string {
  return PROFILES.find((x) => x.profile === p)?.name ?? p;
}

/** A size in bytes, in kB, MB or GB. */
export function bytesWords(n: number): string {
  if (n < 1_000) return `${n} B`;
  if (n < 1_000_000) return `${(n / 1_000).toFixed(0)} kB`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(1)} MB`;
  return `${(n / 1_000_000_000).toFixed(2)} GB`;
}

/** A length of time, short. */
export function forWords(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

export type Tone = 'live' | 'off' | 'wait' | 'fault';

/** The chip and the line under it, from the desk's view (none: the desk has not got it yet). */
export function stateOf(d: RtmpDestinationInfo, video: boolean): { chip: string; tone: Tone; line: string } {
  if (!d.enabled) return { chip: 'Off', tone: 'off', line: 'Not sending.' };
  if (!video) return { chip: 'Waiting', tone: 'fault', line: 'The desk runs without video, and RTMP out needs it.' };
  if (!d.key_readable) return { chip: 'Key needed', tone: 'fault', line: 'The station’s key changed, so this stream key cannot be read. Enter it again.' };
  const s = d.desk;
  if (!s) return { chip: 'Starting', tone: 'wait', line: 'The desk has not taken it yet.' };
  switch (s.state) {
    case 'live':
      return { chip: 'Live', tone: 'live', line: `Sending for ${forWords(s.connected_ms ?? 0)}, ${bytesWords(s.bytes)} so far.` };
    case 'connecting':
      return { chip: 'Connecting', tone: 'wait', line: 'Connecting to the server.' };
    case 'waiting':
      return { chip: 'Starting', tone: 'wait', line: 'Waiting for the encoder’s first picture.' };
    case 'retrying':
      return {
        chip: 'Retrying',
        tone: 'fault',
        // The desk's own words, then a way forward (review U2).
        line: `${s.last_error ?? 'It failed.'} Trying again in ${forWords(s.retry_in_ms ?? 0)}. Check the server and the stream key, and that the platform is waiting for a stream.`,
      };
    default:
      return { chip: 'Off', tone: 'off', line: 'Not sending.' };
  }
}

/** The form's own checks; `null` when it looks right. */
export function urlProblem(url: string): string | null {
  const u = url.trim();
  if (!/^rtmps?:\/\/[^/:\s][^\s]*$/.test(u)) return 'Paste the server URL as the platform shows it: it starts rtmp:// or rtmps://.';
  if (/[?#@]/.test(u)) return 'The server URL has no key in it: the key goes in its own field.';
  return null;
}

export function keyProblem(key: string, editing: boolean): string | null {
  const k = key.trim();
  if (k === '') return editing ? null : 'Paste the stream key the platform shows.';
  if (/\s/.test(k)) return 'The stream key has no spaces.';
  return null;
}

export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (n === '' || n.length > 40 || !/^[\p{L}\p{N} ._-]+$/u.test(n)) return 'Name it in up to 40 letters, digits, spaces, dots, dashes or underscores.';
  return null;
}
