// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The one-key handover (⇧⏎, ADR-0074, ADR-0231): what the next press does, worked out from the
// desk's sources alone, so the key, the button and the tests agree. Two presses with an overlap
// between them, the same two steps the how-to teaches with `5` and `1`:
//   1. the next DJ, waiting off air, goes on air beside the presenter (the overlap);
//   2. everyone presenting who is not that DJ comes off air. The presenter's seat then follows
//      the air to the DJ (the control plane's rule), and with it the talk keys and ⌃T.
// Guests and callers on air are left as they are: they stay on with the next DJ.
// The next show's host in their own studio (ADR-0280; design sections 3.3 and 6) is handed over
// to by the same two presses: the key picks that source, standing by, before any DJ waiting.

import type { Source } from './api.ts';
import { firstName } from './format.ts';

export type HandoverStep =
  /** No DJ in the room to hand over to. */
  | { kind: 'none' }
  /** Put the DJ on air: the overlap starts. */
  | { kind: 'on'; dj: Source }
  /** Take the outgoing presenters off: the DJ holds the air. */
  | { kind: 'off'; dj: Source; off: Source[] }
  /** The DJ already holds the air alone. */
  | { kind: 'done'; dj: Source }
  /** A host's own studio, standing by after its post with nobody presenting (ADR-0280; design
   *  sections 3.5 and 6): one press, and it takes the air from automation. `key` is its show
   *  key; `counting` is the late count running, which would do the same by itself. */
  | { kind: 'go'; dj: Source; key: string; counting: boolean };

/** Who holds the air and comes off at the second press: a host's own studio presents too. */
const presenting = (s: Source) => s.role === 'presenter' || s.role === 'dj' || s.role === 'source';
/** Who the air can be handed to. */
const takes = (s: Source) => s.role === 'dj' || s.role === 'source';

/**
 * @param sources the desk's sources (any order)
 * @param seat the presenter's seat (`view.seats.presenter`)
 * @param pending the DJ this studio app put on air with its last first press, if any: with two
 *   DJs on air it says which one is incoming, whatever their slots.
 * @param go a host's own studio that one press puts on air from automation (`goNow`)
 * @param next the next show's source standing by while someone presents (`handNext`)
 */
export function handoverStep(
  sources: Source[],
  seat: string | null,
  pending: string | null = null,
  go: { source: Source; key: string; counting: boolean } | null = null,
  next: Source | null = null,
): HandoverStep {
  // The next show's source comes before any DJ waiting, and with nobody to take off it is one press.
  if (go) return { kind: 'go', dj: go.source, key: go.key, counting: go.counting };
  const here = sources.filter((s) => s.present).sort((a, b) => a.slot - b.slot);
  const djs = here.filter((s) => s.role === 'dj' && s.identity !== seat);
  // 1. The next show's source standing by, else a DJ waiting off air, other than whoever holds the seat.
  const waiting = (next ? here.find((s) => s.identity === next.identity && !s.on_air) : undefined) ?? djs.find((s) => !s.on_air);
  // 2. The incoming DJ on air: the one this app put on (even if, between two DJs, the seat went
  //    to them by slot order), else a DJ on air who does not hold the seat.
  const incoming =
    here.find((s) => s.identity === pending && takes(s) && s.on_air) ?? djs.find((s) => s.on_air);
  if (incoming) {
    const off = here.filter((s) => s.on_air && presenting(s) && s.identity !== incoming.identity);
    return off.length ? { kind: 'off', dj: incoming, off } : { kind: 'done', dj: incoming };
  }
  if (waiting) return { kind: 'on', dj: waiting };
  // The seat holder is a DJ on air with nobody else presenting: the handover is done.
  const holder = here.find((s) => s.identity === seat);
  if (holder?.role === 'dj' && holder.on_air && !here.some((s) => s.on_air && presenting(s) && s !== holder)) {
    return { kind: 'done', dj: holder };
  }
  return { kind: 'none' };
}

/** The key that runs the handover, as its key cap shows it (`packages/ui/keymap.ts`). A rundown
 *  row with this in its `key` is the handover's row, and carries the button. */
export const HANDOVER_KEY = '⇧⏎';

/** The handover button, on the DJ's tile and the rundown's row: what the next press does, in
 *  the words of its label, with the line beside it saying what follows. Null when there is
 *  nothing to press (no DJ in the room, or the DJ already holds the air). */
export interface HandoverButton {
  kind: 'on' | 'off' | 'go';
  /** The DJ's identity: the tile that carries the button. */
  dj: string;
  label: string;
  /** What the press does, beside the button. */
  then: string;
}

const first = (s: Source) => firstName(s.name, s.identity);

export function handoverButton(step: HandoverStep): HandoverButton | null {
  if (step.kind === 'on') {
    return { kind: 'on', dj: step.dj.identity, label: `Bring ${first(step.dj)} on air`, then: 'On air beside you: the overlap.' };
  }
  if (step.kind === 'go') {
    // While the station counts, the press is the count's own Go on air now; once the count was
    // stopped (Not yet) it is the handover's first press, with nobody to take off.
    return {
      kind: 'go',
      dj: step.dj.identity,
      label: step.counting ? 'Go on air now' : `Bring ${first(step.dj)} on air`,
      then: `${first(step.dj)} takes the air from automation at once.`,
    };
  }
  if (step.kind === 'off') {
    const off = step.off.map(first).join(' and ');
    return { kind: 'off', dj: step.dj.identity, label: `Hand over to ${first(step.dj)}`, then: `${off} ${step.off.length === 1 ? 'comes' : 'come'} off air.` };
  }
  return null;
}
