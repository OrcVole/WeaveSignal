// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A live level for one microphone track, for the microphone check and the call panel.

import { barPct, dbfs, heard } from './meter.ts';

export class Meter {
  /** The bar's fill, 0 to 100. */
  pct = $state(0);
  /** The microphone has been heard since the check started. */
  heard = $state(false);
  #ctx: AudioContext | null = null;
  #raf = 0;

  constructor(track: MediaStreamTrack) {
    try {
      const ctx = new AudioContext();
      const src = ctx.createMediaStreamSource(new MediaStream([track]));
      const an = ctx.createAnalyser();
      an.fftSize = 2048;
      src.connect(an);
      const buf = new Float32Array(an.fftSize);
      let peak = -Infinity;
      const tick = () => {
        an.getFloatTimeDomainData(buf);
        const db = dbfs(buf);
        peak = Math.max(db, peak - 0.5);
        this.pct = barPct(db);
        if (heard(peak)) this.heard = true;
        this.#raf = requestAnimationFrame(tick);
      };
      this.#ctx = ctx;
      tick();
    } catch {
      // No Web Audio: the bar stays empty; the call itself does not need it.
    }
  }

  stop(): void {
    cancelAnimationFrame(this.#raf);
    void this.#ctx?.close().catch(() => {});
    this.#ctx = null;
  }
}
