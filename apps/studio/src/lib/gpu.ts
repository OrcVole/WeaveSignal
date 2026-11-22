// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A graphics card for the station's video, as Settings says it (1.2.1, ADR-0292). The station
// reports what its install knows (`install.gpu` in `GET /api/v1/setup`): whether the server has
// an NVIDIA card the desk could be given, whether the desk has been given it, and whether video
// is on. It does not know the card's model, so it never says the card can encode: only a card
// with an AV1 encoder takes the station's video, and the desk falls back to the processor. This puts it into plain words and, where the card is there and not
// used, offers it.
//
// The offer is two steps on the server, not a switch here. A card is handed to the desk when
// its container is made, which only the install script does (`GPU=nvidia` in the settings file,
// then `weavesignal.sh`); the studio cannot write that file, as it cannot write `STUN_SERVERS`
// on Compose or the Quadlets. So nothing here turns a card on, and an update never does: a
// station installed before 1.2 keeps encoding on the processor until someone takes the steps.
//
// A station older than 1.2.1 sends no `gpu`, the Cloudron package says it has none (an app
// there is given no card), and a server with no card has none to offer: nothing is drawn.

import { base, headers, netFetch } from './api.ts';

/** `install.gpu` in the setup's view. */
export interface GpuFacts {
  /** The install script found an NVIDIA card on the server (its CDI spec). */
  found: boolean;
  /** The desk has been handed the card (`GPU=nvidia`). */
  in_use: boolean;
  /** Video is on (`VIDEO=true`). */
  video: boolean;
}

export interface GpuWords {
  /** `offered`: there, not used. `in_use`: the desk has it, with video on. `ready`: the desk
   *  has it, and video is off. */
  state: 'offered' | 'in_use' | 'ready';
  /** The short word beside the title. */
  tag: string;
  /** The first sentence, in bold: what is so. */
  lead: string;
  /** What that means. */
  rest: string;
  /** The steps that put the card to use; null when there is nothing to do. */
  steps: { text: string; code: string | null }[] | null;
  /** The quiet line under the steps: nothing changes unasked, and the way back. */
  after: string;
}

const RUN = 'Run weavesignal.sh again, from the folder it is in. On the Quadlets, generate the units again after it.';
/** What the station cannot know from here: whether this card has the encoder. The install script
 *  finds a card, not its model, and only a card with an AV1 encoder takes the station's video. */
const IF_IT_CAN = 'A card with an AV1 encoder (NVIDIA’s RTX 40 series or later) takes it; with an older card the desk stays on the processor and says so in its log.';

/** The panel's words, or null when there is no card to speak of. They say what the install
 *  knows (a card is there; the desk has it or not), never that the card is encoding: that is
 *  the desk's to say, in its log. */
export function gpuWords(g: GpuFacts | null | undefined): GpuWords | null {
  if (!g || !g.found) return null;
  if (g.in_use) {
    return g.video
      ? {
          state: 'in_use',
          tag: 'With the desk',
          lead: 'The desk has been given this server’s NVIDIA graphics card.',
          rest: `With video on, it encodes on the card (NVENC) what the card takes, and the rest on the processor. ${IF_IT_CAN}`,
          steps: null,
          after: 'To put all of it back on the processor, set GPU=none in the station’s settings file on the server and run weavesignal.sh again.',
        }
      : {
          state: 'ready',
          tag: 'With the desk · video off',
          lead: 'The desk has been given this server’s NVIDIA graphics card.',
          rest: `Video is off, so nothing is encoded yet. With video on (VIDEO=true in the station’s settings file) the desk encodes on the card (NVENC) what the card takes. ${IF_IT_CAN}`,
          steps: null,
          after: 'To keep the card from the desk, set GPU=none in the station’s settings file on the server and run weavesignal.sh again.',
        };
  }
  return {
    state: 'offered',
    tag: 'Not in use',
    lead: 'This server has an NVIDIA graphics card, and the station is not using it.',
    rest: g.video
      ? `Video is encoded on the processor (SVT-AV1). The desk can encode it on the card instead (NVENC), which costs the processor far less and leaves it for the sound. ${IF_IT_CAN}`
      : `Video is off. With video on, the desk can encode the picture on the card (NVENC) instead of the processor. ${IF_IT_CAN}`,
    steps: [
      { text: g.video ? 'On the server, in the station’s settings file, set' : 'On the server, in the station’s settings file, turn video on and set', code: g.video ? 'GPU=nvidia' : 'VIDEO=true and GPU=nvidia' },
      { text: RUN, code: null },
    ],
    after: 'Nothing changes until you do this: an update never moves the station to the card. To go back, set GPU=none and run the script again.',
  };
}

/** What the install knows of a card; null on a station older than 1.2.1, or where it cannot be
 *  read (the setup's view is an admin's). */
export async function gpuFacts(): Promise<GpuFacts | null> {
  const res = await netFetch(`${base()}/api/v1/setup`, { method: 'GET', headers: headers(false) });
  if (!res.ok) return null;
  const data = (await res.json().catch(() => ({}))) as { install?: { gpu?: Partial<GpuFacts> } };
  const g = data.install?.gpu;
  return g && typeof g.found === 'boolean' ? { found: g.found, in_use: !!g.in_use, video: !!g.video } : null;
}
