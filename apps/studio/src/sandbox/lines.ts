// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What the sandbox's people say (ADR-0212, ADR-0222): a line is one recorded clip (or one not
// yet recorded, shown as a caption until it is), and a beat is a run of lines that starts when
// its condition holds. The scenarios are built from these; the station's director plays them.

import type { Station } from './station.ts';

/** Where a line is heard: on the desk like any voice (`air`), or in a caller's booth with the
 *  screener (`booth`), which never reaches the programme. */
export type Heard = 'air' | 'booth';

export interface Line {
  id: string;
  /** The speaker's identity in the room. */
  who: string;
  /** Its length in ms (the stems' `check.tsv`; an estimate for a line not yet recorded). */
  ms: number;
  text: string;
  heard: Heard;
  /** Skipped, not waited for, when its speaker cannot be heard. */
  optional?: boolean;
  /** False for a line written but not yet recorded: no clip is fetched, the caption carries it
   *  (`lines-to-record.tsv` lists them for the recording session). */
  clip?: boolean;
}

/** A recorded line: its length is the clip's. */
export const L = (id: string, who: string, s: number, text: string, heard: Heard = 'air', optional = false): Line => ({
  id,
  who,
  ms: Math.round(s * 1000),
  text,
  heard,
  optional,
});

/** About how long a line takes to say, from its text: fifteen characters a second and a breath. */
export function estimateMs(text: string): number {
  return Math.round(text.length * 65 + 400);
}

/** A line written for a later recording session: shown as a caption, timed from its text. */
export const U = (id: string, who: string, text: string, heard: Heard = 'air', optional = false): Line => ({
  id,
  who,
  ms: estimateMs(text),
  text,
  heard,
  optional,
  clip: false,
});

/** A run of lines that starts when its condition holds, once. A beat with no lines is a cue:
 *  its `after` runs as soon as its condition holds (the simulated presenter acting). */
export interface Beat {
  id: string;
  when: (s: Station) => boolean;
  lines: Line[];
  /** What happens when the beat's last line has played. */
  after?: (s: Station, now: number) => void;
}

/** A line being spoken: who, which, and until when. */
export interface Speaking {
  line: Line;
  from: number;
  until: number;
}
