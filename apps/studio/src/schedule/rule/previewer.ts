// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Asking the control plane what a rule makes (new-show.md section 8): 250 ms after the last
// change, and any answer older than the newest request is dropped. While an answer is awaited
// the last one stays, dimmed, and *Working out the dates…* is said after 400 ms. A station that
// cannot be reached keeps the answers typed and offers Try again. No page here: the fetch, the
// clock and the listener are given, so the timing is tested on its own.

import { FieldError, type RulePreview, type RulePreviewRequest } from '../../lib/api.ts';

export interface PreviewState {
  /** The newest answer, or null before the first. */
  answer: RulePreview | null;
  /** The answer shown is older than the questions: drawn dimmed. */
  stale: boolean;
  /** Say *Working out the dates…*. */
  working: boolean;
  /** The station cannot be reached, so the dates cannot be shown. */
  unreachable: boolean;
}

export const EMPTY_PREVIEW: RulePreview = {
  sentence: '',
  short: '',
  last: null,
  dates: [],
  more: null,
  total: null,
  clear: null,
  problems: [],
  hints: [],
  dropped: { skip: [], moves: [] },
  change: null,
};

export interface PreviewerOptions {
  /** How long after the last change the station is asked. */
  delay?: number;
  /** How long an answer may take before the wait is said. */
  sayAfter?: number;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (t: unknown) => void;
}

export class Previewer {
  private state: PreviewState = { answer: null, stale: false, working: false, unreachable: false };
  private seq = 0;
  private wait: unknown = null;
  private slow: unknown = null;
  private last: RulePreviewRequest | null = null;
  private lastKey = '';
  private readonly delay: number;
  private readonly sayAfter: number;
  private readonly setTimer: (fn: () => void, ms: number) => unknown;
  private readonly clearTimer: (t: unknown) => void;

  constructor(
    private readonly ask: (req: RulePreviewRequest) => Promise<RulePreview>,
    private readonly tell: (s: PreviewState) => void,
    o: PreviewerOptions = {},
  ) {
    this.delay = o.delay ?? 250;
    this.sayAfter = o.sayAfter ?? 400;
    this.setTimer = o.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = o.clearTimer ?? ((t) => clearTimeout(t as ReturnType<typeof setTimeout>));
  }

  /** The answer shown before the station has been asked (a test's, or a dialog reopened). */
  seed(answer: RulePreview | null): void {
    this.set({ answer, stale: false, working: false, unreachable: false });
  }

  /** The questions changed. `null`: they do not make a rule yet, so nothing is asked and the
   *  last answer stays dimmed. `now` asks without the wait (the first time, and Try again). */
  changed(req: RulePreviewRequest | null, now = false): void {
    const key = req ? JSON.stringify(req) : '';
    if (key && key === this.lastKey && !this.state.unreachable && !now) return;
    this.lastKey = key;
    this.last = req;
    this.cancel();
    // A newer question makes any answer still on its way an old one.
    this.seq++;
    if (!req) return this.set({ ...this.state, stale: this.state.answer !== null, working: false });
    this.set({ ...this.state, stale: true, working: false });
    if (now) this.send(req);
    else this.wait = this.setTimer(() => this.send(req), this.delay);
  }

  /** Try again, after the station could not be reached. */
  retry(): void {
    if (this.last) this.changed(this.last, true);
  }

  /** The dialog closed: nothing more is told. */
  stop(): void {
    this.cancel();
    this.seq++;
  }

  private cancel(): void {
    if (this.wait !== null) this.clearTimer(this.wait);
    if (this.slow !== null) this.clearTimer(this.slow);
    this.wait = this.slow = null;
  }

  private send(req: RulePreviewRequest): void {
    const mine = ++this.seq;
    this.wait = null;
    this.slow = this.setTimer(() => {
      if (mine === this.seq) this.set({ ...this.state, working: true });
    }, this.sayAfter);
    this.ask(req).then(
      (answer) => {
        if (mine !== this.seq) return;
        this.cancel();
        this.set({ answer, stale: false, working: false, unreachable: false });
      },
      (e: unknown) => {
        if (mine !== this.seq) return;
        this.cancel();
        // A refusal in the station's words (a rule its check refuses) is an answer: the
        // problem, by its field. Anything else is a station that cannot be reached.
        if (e instanceof FieldError && (e.status === 400 || e.status === 409 || e.status === 422)) {
          this.set({ answer: { ...EMPTY_PREVIEW, problems: [{ field: e.field ?? '', message: e.message }] }, stale: false, working: false, unreachable: false });
        } else {
          this.set({ ...this.state, stale: this.state.answer !== null, working: false, unreachable: true });
        }
      },
    );
  }

  private set(s: PreviewState): void {
    this.state = s;
    this.tell(s);
  }
}
