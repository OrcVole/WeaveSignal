// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A person's own picture before they join (ADR-0171). The camera is opened here and shown to
// them alone: nothing is sent to the studio until they press join, when the same live track is
// handed to the call, ready to publish. A camera that will not start costs only the picture.

import type { LocalVideoTrack } from 'livekit-client';
import { cameraProblem } from './mic.ts';
import { cameraCapture } from './rules.ts';

/** Opens the camera at the desk's picture size and rate. */
export type Capture = () => Promise<LocalVideoTrack>;

const capture: Capture = async () => {
  const { createLocalVideoTrack } = await import('livekit-client');
  return createLocalVideoTrack(cameraCapture());
};

/** What the preview hands to the call at join. */
export interface CameraHandover {
  /** The live track the person has been looking at, or null if there is none. */
  track: LocalVideoTrack | null;
  /** Whether it goes on once they are in (the default); false joins with the camera off. */
  on: boolean;
  /** The sentence for a camera that did not start, worded for inside the call, or null. */
  problem: string | null;
}

export class CameraPreview {
  /** The track to show in the preview; null before it starts, after a failure, and once handed over. */
  shown = $state<MediaStreamTrack | null>(null);
  starting = $state(false);
  problem = $state<string | null>(null);
  /** Whether the camera is on once they join. On by default (ADR-0171). */
  sendOnJoin = $state(true);

  #track: LocalVideoTrack | null = null;
  #open: Capture;
  #error: unknown = null;

  constructor(open: Capture = capture) {
    this.#open = open;
  }

  /** Ask for the camera and show it. Safe to call again after a failure. */
  async start(): Promise<void> {
    if (this.starting || this.#track) return;
    this.starting = true;
    this.problem = null;
    this.#error = null;
    try {
      const t = await this.#open();
      this.#track = t;
      this.shown = t.mediaStreamTrack;
    } catch (e) {
      this.#error = e ?? new Error('camera');
      this.problem = cameraProblem(e);
    } finally {
      this.starting = false;
    }
  }

  /** Let go of the camera (the person left the page without joining). */
  stop(): void {
    this.#track?.stop();
    this.#track = null;
    this.shown = null;
  }

  /** Give the call the live track and what the person chose; the preview no longer owns it. */
  handOver(): CameraHandover {
    const live = this.#track && this.#track.mediaStreamTrack.readyState === 'live' ? this.#track : null;
    if (this.#track && !live) this.#track.stop();
    const out = { track: live, on: this.sendOnJoin, problem: this.#error ? cameraProblem(this.#error, true) : null };
    this.#track = null;
    this.shown = null;
    return out;
  }
}
