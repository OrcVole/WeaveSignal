// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The listener streams a station offers (ADR-0232): Opus first, then each MP3 stream it turns
// on, lowest rate first. The listener chooses; the choice is kept on this device, where browser
// storage allows (every read and write is guarded, and the page works without it).

export interface StreamChoice {
  codec: 'opus' | 'mp3';
  /** Constant bit rate, in kbit/s. */
  kbps: number;
  url: string;
}

const KEY = 'weavesignal.stream';

/** "Opus, 96 kbit/s". */
export function label(s: StreamChoice): string {
  return `${s.codec === 'opus' ? 'Opus' : 'MP3'}, ${s.kbps} kbit/s`;
}

/** A few words on when to choose it, or null. */
export function hint(s: StreamChoice, all: StreamChoice[]): string | null {
  if (s.codec === 'opus') return 'The best sound for its size.';
  const mp3 = all.filter((x) => x.codec === 'mp3');
  if (mp3.length > 1 && s.kbps === Math.max(...mp3.map((x) => x.kbps))) return 'For music. Uses the most data.';
  return 'For players that cannot play Opus.';
}

/** The stream to play: the listener's own choice while the station still offers it; else Opus
 * where this browser plays it; else the first MP3; else the station's one stream address. */
export function pick(streams: StreamChoice[], chosen: string | null, fallback: string, opusOk = true): string {
  if (chosen && streams.some((s) => s.url === chosen)) return chosen;
  const first = opusOk ? streams[0] : (streams.find((s) => s.codec === 'mp3') ?? streams[0]);
  return first?.url ?? fallback;
}

export function remembered(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function remember(url: string): void {
  try {
    localStorage.setItem(KEY, url);
  } catch {
    // Refused: the choice lasts as long as the page.
  }
}
