// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The browser's microphones, kept current while a page shows them (1.3.3): read once the page
// has been given a microphone (a browser names none before), and again whenever one is plugged
// in or taken away.

import { type Listed, type Mic, inUse, listed, mics } from './mic-devices.ts';

export class MicList {
  /** The microphones to choose from; empty until the browser names them. */
  list = $state<Mic[]>([]);
  #all: Listed[] = [];
  #watching = false;
  #onChange: (() => void) | null = null;

  /** Read the list again; `onChange` (set by `watch`) hears of it when the browser reports a change. */
  async refresh(): Promise<Mic[]> {
    this.#all = await listed();
    this.list = mics(this.#all);
    return this.list;
  }

  /** The microphone a track is on, as the list names it. */
  of(track: MediaStreamTrack | null | undefined): Mic | null {
    if (!track) return null;
    return inUse(track.getSettings(), track.label, this.#all);
  }

  /** Follow microphones plugged in and taken away. `then` runs after each new list. */
  watch(then?: () => void): void {
    this.#onChange = then ?? null;
    if (this.#watching) return;
    this.#watching = true;
    globalThis.navigator?.mediaDevices?.addEventListener?.('devicechange', this.#changed);
  }

  stop(): void {
    this.#watching = false;
    this.#onChange = null;
    globalThis.navigator?.mediaDevices?.removeEventListener?.('devicechange', this.#changed);
  }

  #changed = (): void => {
    void this.refresh().then(() => this.#onChange?.());
  };
}
