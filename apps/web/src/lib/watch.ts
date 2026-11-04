// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the player plays (M5): the desk's HLS with its picture, the same HLS's sound alone, or
// the Icecast stream, chosen by what the station offers and what this browser can decode.
//
// - Video: AV1 and Opus in fMP4, played by script (hls.js over Media Source Extensions, or
//   Safari's ManagedMediaSource): Safari's own HLS player does not play Opus (ADR-0025).
// - Audio only, by choice (the toggle, ADR-0026), or because the device cannot decode AV1:
//   older Apple devices get the sound and the stills (ADR-0024). Same segments, same clock, so
//   switching back does not jump.
// - Icecast, where there is no video, no script-based playback, or no Opus in fMP4.

/** What this browser can do, as far as the player cares. */
export interface Caps {
  /** Media Source Extensions (or Safari's ManagedMediaSource). */
  mse: boolean;
  /** AV1 in fMP4, at the default rung's level. */
  av1: boolean;
  /** Opus in fMP4. */
  opusMp4: boolean;
}

export type Mode = 'video' | 'audio' | 'icecast';

export interface Choice {
  mode: Mode;
  /** The address to load: an HLS playlist, or the Icecast stream. */
  url: string;
  /** Why not the picture, in a sentence for the listener; null when there is nothing to say. */
  note: string | null;
  /** Whether the audio-only toggle can be offered. */
  canToggle: boolean;
}

export interface VideoInfo {
  live: string;
  audio: string;
}

/** The AV1 codec string of the desk's 480p rungs, and Opus, as MSE names them. */
export const AV1_TYPE = 'video/mp4; codecs="av01.0.04M.08"';
export const OPUS_TYPE = 'audio/mp4; codecs="opus"';

type MS = { isTypeSupported(t: string): boolean };

/** Read the browser's abilities (in a page; `win` for tests). */
export function detect(win: unknown = globalThis): Caps {
  const w = win as { MediaSource?: MS; ManagedMediaSource?: MS };
  const ms = w.ManagedMediaSource ?? w.MediaSource;
  if (!ms || typeof ms.isTypeSupported !== 'function') return { mse: false, av1: false, opusMp4: false };
  return { mse: true, av1: ms.isTypeSupported(AV1_TYPE), opusMp4: ms.isTypeSupported(OPUS_TYPE) };
}

/** What to play, given the station's offer, the browser, and the listener's toggle. */
export function choose(video: VideoInfo | null, streamUrl: string, caps: Caps, audioOnly: boolean): Choice {
  const icecast = (note: string | null): Choice => ({ mode: 'icecast', url: streamUrl, note, canToggle: false });
  if (!video) return icecast(null);
  if (!caps.mse || !caps.opusMp4) {
    return icecast("This browser can't play the station's picture, so you hear the station's audio stream.");
  }
  if (!caps.av1) {
    return {
      mode: 'audio',
      url: video.audio,
      note: "This device can't show the station's picture (it needs AV1), so you hear the sound only.",
      canToggle: false,
    };
  }
  return audioOnly
    ? { mode: 'audio', url: video.audio, note: null, canToggle: true }
    : { mode: 'video', url: video.live, note: null, canToggle: true };
}

/** A fragment of a loaded playlist, as hls.js describes it. */
export interface Frag {
  start: number;
  duration: number;
  programDateTime: number | null;
}

/**
 * Where on the newly loaded playlist the moment `at` (ms since the epoch, from the playlist
 * the listener left) is: the same instant by the segments' program date-time, so switching to
 * audio only and back neither jumps nor repeats. `null` if the playlist no longer holds it.
 */
export function positionAt(frags: Frag[], at: number): number | null {
  for (const f of frags) {
    if (f.programDateTime === null) continue;
    const off = (at - f.programDateTime) / 1000;
    if (off >= 0 && off < f.duration) return f.start + off;
  }
  return null;
}
