// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// One slate still's control, shared by the desk's station still (desk.picture) and a show's
// own still in the show editor (calendar.show-still, ADR-0200): what the station has, a
// picture being sent or taken away, and the station's sentence when it refuses one.

import { NO_STILL, putStill, removeStill, stillImage, stillInfo, stillProblem, type StillInfo } from './video.ts';

export type StillPhase = '' | 'looking' | 'sending' | 'removing';

export class StillControl {
  /** What the station has; null until the first answer. */
  info = $state<StillInfo | null>(null);
  /** The still as the desk shows it, a `data:` URL, when a preview was asked for. */
  image = $state<string | null>(null);
  phase = $state<StillPhase>('');
  /** The station's sentence when it refused the last request; empty otherwise. */
  problem = $state('');

  /**
   * @param show the show's slug, or none for the station's still
   * @param preview whether to fetch the picture itself for a preview
   */
  constructor(
    readonly show?: string,
    readonly preview = false,
  ) {}

  /** Working: the controls are held meanwhile. */
  get busy(): boolean {
    return this.phase !== '';
  }

  /** Ask the station what it has; a failed look leaves nothing set and says nothing. */
  async load(): Promise<void> {
    this.phase = 'looking';
    try {
      this.info = await stillInfo(this.show);
      this.image = this.preview && this.info.present ? await stillImage(this.show) : null;
    } catch {
      this.info = null;
      this.image = null;
    } finally {
      this.phase = '';
    }
  }

  /** Send a picture; the station's sentence stays in `problem` if it is refused. */
  async choose(file: Blob): Promise<void> {
    this.phase = 'sending';
    this.problem = '';
    try {
      this.info = await putStill(file, this.show);
      if (this.preview) this.image = await stillImage(this.show);
    } catch (err) {
      this.problem = stillProblem(err);
    } finally {
      this.phase = '';
    }
  }

  /** Take the picture away. */
  async remove(): Promise<void> {
    this.phase = 'removing';
    this.problem = '';
    try {
      await removeStill(this.show);
      this.info = NO_STILL;
      this.image = null;
    } catch (err) {
      this.problem = stillProblem(err);
    } finally {
      this.phase = '';
    }
  }
}
