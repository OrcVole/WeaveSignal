// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The monitor link's connection to its station: the view as it arrives, the station's clock,
// and whether the page can still vouch for what it shows.
//
// The station sends a view every second. A page that has heard nothing for a few seconds does
// not know the host's state any more, whether the stream failed or only went quiet, so it says
// it has lost the station and shows no state at all until a view arrives again (the design's
// section 9: "it never shows a stale ON AIR").

import { type Done, type MonitorView, type Press, press, readEvents } from './api.ts';
import { ApiError } from '../api.ts';
import { StationClock } from './clock.ts';

/** Where the page stands with its station. */
export type Standing =
  /** No view has arrived yet. */
  | 'opening'
  /** Views are arriving: what the page shows is the station's word, a second old at most. */
  | 'live'
  /** No view for a few seconds: the page cannot say what state the host is in. */
  | 'lost'
  /** The station refuses the page for now (too many pages, or too many wrong links). */
  | 'refused'
  /** The link is not good: there is no secret in it, the secret is wrong, or its key has ended. */
  | 'gone';

/** A view older than this is not shown as the host's state. The station sends one a second. */
export const STALE_MS = 3500;

/** How a press ended, for the controls. */
export type Pressed =
  | { ok: true; done: Done }
  /** The station answered that it would not, or could not, in its own words. */
  | { ok: false; refused: string }
  /** The station did not answer: the page does not know whether it happened. */
  | { ok: false; unanswered: true }
  /** The link is not good any more. */
  | { ok: false; gone: true };

export class MonitorLink {
  standing = $state<Standing>('opening');
  /** The last view. Only the station's word while `standing` is `live`. */
  view = $state<MonitorView | null>(null);
  /** The station's sentence when the link is gone or refused. */
  words = $state('');
  /** The station's time now, ticking; null until the station has said. */
  now = $state<number | null>(null);
  /** The station's time when the last view arrived. */
  heardAt = $state<number | null>(null);

  readonly secret: string;
  #clock = new StationClock();
  #heardSteady = 0;
  #openedSteady = 0;
  #stop: AbortController | null = null;
  #stream: AbortController | null = null;
  #tick: ReturnType<typeof setInterval> | undefined;
  #onGone: () => void;

  constructor(secret: string, onGone: () => void = () => {}) {
    this.secret = secret;
    this.#onGone = onGone;
  }

  start(): void {
    // A secret is letters and digits; anything a header cannot carry is not one.
    if (!/^[\x21-\x7e]{1,200}$/.test(this.secret)) {
      this.#gone('This page needs the link the station sent you. Open that link, all of it, on this device.');
      return;
    }
    this.#stop = new AbortController();
    this.#openedSteady = performance.now();
    this.#tick = setInterval(() => this.#beat(), 200);
    void this.#run(this.#stop.signal);
  }

  stop(): void {
    clearInterval(this.#tick);
    this.#stop?.abort();
    this.#stream?.abort();
  }

  /** One press of a control. A press the station answered brings its view with it. */
  async press(route: Press, body?: Record<string, unknown>): Promise<Pressed> {
    try {
      const done = await press(this.secret, route, body);
      this.#take(done.view);
      return { ok: true, done };
    } catch (e) {
      if (e instanceof ApiError) {
        if (e.status === 401 || e.status === 410) {
          this.#gone(e.message);
          return { ok: false, gone: true };
        }
        // The station answered, in words: a refusal (409), or that it cannot do it just now.
        if (e.message) return { ok: false, refused: e.message };
      }
      return { ok: false, unanswered: true };
    }
  }

  /** A view arrived, from the stream or with a press's answer. */
  #take(v: MonitorView): void {
    if (this.standing === 'gone') return;
    const steady = performance.now();
    this.#clock.read(v.station.now_ms, steady);
    this.#heardSteady = steady;
    this.now = this.#clock.at(steady);
    this.heardAt = this.now;
    this.standing = 'live';
    this.words = '';
    // A view made a moment before the one on the page (the stream's, overtaken by a press's
    // answer) is proof the station is there, and no more: the lamp never steps back to an older
    // state. A view from further back is not one overtaken on the way: the station's clock was
    // set back, and what it says now is the state now.
    const back = this.view ? this.view.station.now_ms - v.station.now_ms : 0;
    if (back > 0 && back < STALE_MS) return;
    this.view = v;
  }

  /** Five times a second: move the clock on, and notice a stream gone quiet. */
  #beat(): void {
    const steady = performance.now();
    this.now = this.#clock.at(steady);
    const quiet = this.standing === 'live' && steady - this.#heardSteady > STALE_MS;
    // A stream that opens and never says anything is as good as none.
    const mute = this.standing === 'opening' && steady - this.#openedSteady > 2 * STALE_MS;
    if (quiet || mute) {
      this.standing = 'lost';
      // A stream that is open and silent will not end by itself: start again.
      this.#stream?.abort();
    }
  }

  #gone(words: string): void {
    this.standing = 'gone';
    this.words = words;
    this.view = null;
    this.stop();
    this.#onGone();
  }

  async #run(stop: AbortSignal): Promise<void> {
    let tries = 0;
    while (!stop.aborted) {
      const stream = new AbortController();
      this.#stream = stream;
      const before = this.#heardSteady;
      const end = await readEvents(this.secret, stream.signal, (v) => this.#take(v));
      if (stop.aborted) return;
      if (end.kind === 'gone') {
        this.#gone(end.words);
        return;
      }
      // Whatever ended it, the page is not hearing the station now.
      if (end.kind !== 'refused') this.standing = 'lost';
      let wait: number;
      if (end.kind === 'refused') {
        this.standing = 'refused';
        this.words = end.words;
        wait = end.retry_ms;
      } else {
        // A stream that had been carrying views is tried again at once; one that fails over and
        // over is tried after 1, 2, 4, then every 5 seconds.
        tries = this.#heardSteady > before ? 0 : tries + 1;
        wait = tries === 0 ? 300 : Math.min(5000, 1000 * 2 ** (tries - 1));
      }
      await new Promise((r) => setTimeout(r, wait));
    }
  }
}
