// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The state vocabulary as code (design system section 3): one shape, one word, one colour per
// state, and the rules that pick the word from what the desk reports.
//
// ON AIR means only "listeners can hear this" (ADR-0062). While playout is down a source the
// room hears says OPEN, the white lamp (ADR-0079), and the master lamp says PLAYOUT DOWN; the
// master lamp itself never says OPEN. DESK BUSY and BANNED BEFORE are vocabulary (ADR-0069).
//
// In auto mode automation holds the air and the desk never reaches listeners (ADR-0183): the
// master lamp says AUTO, a hollow lamp in the mode colour, and by the same rule an open fader
// says OPEN, the white lamp. AUTO sits above PLAYOUT DOWN: the design reads the mode before
// playout and on-air, and only a desk that does not answer outranks it.

/** A chip: its class on `.state`, its glyph, the glyph's own colour class, and its word. */
export interface Chip {
  cls: string;
  glyph: string;
  glyphCls: string;
  word: string;
}

export const CHIP = {
  onAir: { cls: 'fill-onair', glyph: 'g-onair', glyphCls: '', word: 'On air' },
  open: { cls: 'fill-open', glyph: 'g-open', glyphCls: '', word: 'Open' },
  cue: { cls: 'fill-cue', glyph: 'g-cue', glyphCls: '', word: 'Cue' },
  talk: { cls: 'fill-talk', glyph: 'g-talk', glyphCls: '', word: 'Talk' },
  screening: { cls: 'fill-talk', glyph: 'g-talk', glyphCls: '', word: 'Screening' },
  muted: { cls: 'hollow hollow-off', glyph: 'g-muted', glyphCls: '', word: 'Muted' },
  standby: { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Standby' },
  waiting: { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Waiting' },
  ready: { cls: 'hollow hollow-good', glyph: 'g-good', glyphCls: 'g-good', word: 'Ready' },
  dropped: { cls: 'hollow hollow-off', glyph: 'g-muted', glyphCls: '', word: 'Dropped' },
  allGood: { cls: 'hollow hollow-good', glyph: 'g-good', glyphCls: 'g-good', word: 'All good' },
  fault: { cls: 'hollow hollow-critical', glyph: 'g-fault', glyphCls: 'g-critical', word: 'Fault' },
  playoutDown: { cls: 'hollow hollow-serious', glyph: 'g-playout', glyphCls: 'g-serious', word: 'Playout down' },
  linkDegraded: { cls: 'hollow hollow-warning', glyph: 'g-link-degraded', glyphCls: 'g-warning', word: 'Link degraded' },
  deskBusy: { cls: 'hollow hollow-warning', glyph: 'g-desk-busy', glyphCls: 'g-warning', word: 'Desk busy' },
  defaultLoop: { cls: 'hollow hollow-warning', glyph: 'g-degraded', glyphCls: 'g-warning', word: 'Default loop' },
  bannedBefore: { cls: 'hollow hollow-serious', glyph: 'g-fault', glyphCls: 'g-serious', word: 'Banned before' },
  delay: { cls: 'hollow hollow-accent', glyph: 'g-delay', glyphCls: 'g-accent', word: 'Delay' },
  dumped: { cls: 'hollow hollow-warning', glyph: 'g-dumped', glyphCls: 'g-warning', word: 'Dumped' },
  auto: { cls: 'hollow hollow-auto', glyph: 'g-auto', glyphCls: 'g-auto', word: 'Auto' },
} as const satisfies Record<string, Chip>;

/** The one plain sentence under each state word (design system section 3, "The tooltip on each
 *  word", 2026-11-02): what it means for the person at the desk, and what to do. Keyed by a
 *  chip's own word, not by what a surface prints in its place (the delay chip prints a figure).
 *  Only the fifteen vocabulary words have one: READY, WAITING, SCREENING, DROPPED and DEFAULT
 *  LOOP, and the surface words, have no approved sentence and show none. */
export const STATE_TIPS: Readonly<Record<string, string>> = {
  [CHIP.onAir.word]: 'Listeners can hear this now; speak as if the whole audience is in the room, because it is.',
  [CHIP.open.word]:
    'The room hears this source but listeners do not, because playout or automation holds the air; talk freely, and check the master lamp before you expect to be heard.',
  [CHIP.auto.word]:
    'Automation is playing the log and the desk never reaches listeners; pressing On opens you to the room only, and you leave auto mode before you go on air.',
  [CHIP.cue.word]:
    'This source is in your headphones only; listeners hear nothing from it, so use it to check a caller or a cart before you open the fader.',
  [CHIP.talk.word]: 'Your voice is going to this one person for as long as you hold the key; listeners never hear talkback.',
  [CHIP.muted.word]:
    'The source is cut from every mix; unmute it before you open its fader, or the On key changes nothing anyone hears.',
  [CHIP.standby.word]: 'Nothing is open and playout holds the air; the next On key puts you on air at once.',
  [CHIP.allGood.word]: 'Nothing needs you; every link, mix and output is as it should be.',
  [CHIP.fault.word]:
    'Something the desk depends on has failed and listeners hear the fallback; open the panel the chip sits on, which names what to restart.',
  [CHIP.playoutDown.word]:
    'Playout has stopped while the room is alive; listeners hear the fallback playlist, and your open sources say OPEN; restart playout on the server to bring it back.',
  [CHIP.linkDegraded.word]:
    "This person's connection is losing packets or adding delay and they may sound rough; watch it, and ask them to change network if it does not clear.",
  [CHIP.delay.word]:
    'The profanity delay is rebuilding after a dump and the figure is how far behind you listeners are now; Dump protects you only once it is full again.',
  [CHIP.dumped.word]:
    'The last seconds were cut from the stream and listeners heard the dump; the delay is rebuilding, so carry on and do not repeat the words.',
  [CHIP.deskBusy.word]:
    'The presenter declined the last send; the caller stays ready in the queue, so hold them and offer again at the next gap.',
  [CHIP.bannedBefore.word]:
    "This caller's account was banned before, with the date and reason shown; nothing is on air yet, so screen them with that in mind or decline them.",
};

/** A chip's tooltip, or null for a word the design has no sentence for. */
export function tipFor(chip: Chip): string | null {
  return STATE_TIPS[chip.word] ?? null;
}

/** The station's mode (ADR-0045), as the studio view carries it (`mode`): `live`, the desk first
 *  with automation on dead air, or `auto`, automation holding the air, where the master lamp is
 *  to say AUTO (ADR-0183). Null until the control plane has read the setting. */
export type StationMode = 'live' | 'auto';

/** The master lamp. */
export type LampState = 'on-air' | 'auto' | 'playout-down' | 'standby' | 'fault';

export interface LampFacts {
  /** The desk answers the control plane. */
  deskReachable: boolean;
  /** Playout is connected to the desk's programme output: listeners can hear the room. */
  playoutConnected: boolean;
  /** At least one source is on air and not muted. */
  anyOnAir: boolean;
  /** The station's mode (`View.mode`); absent or null until the control plane has read it,
   *  which reads as `live`. */
  mode?: StationMode | null;
}

/** Whether listeners can hear the room: playout is connected and automation does not hold the
 *  air. What decides ON AIR against OPEN on a source (ADR-0062, ADR-0183). */
export function listenersHear(playoutConnected: boolean, mode?: StationMode | null): boolean {
  return playoutConnected && mode !== 'auto';
}

/** What the master lamp says. A desk that does not answer is a fault, whatever else is known;
 *  then automation holding the air is AUTO (ADR-0183), before playout and on-air are read;
 *  without playout listeners hear nothing of the room, so PLAYOUT DOWN, whoever is open. */
export function masterLamp(f: LampFacts): LampState {
  if (!f.deskReachable) return 'fault';
  if (f.mode === 'auto') return 'auto';
  if (!f.playoutConnected) return 'playout-down';
  return f.anyOnAir ? 'on-air' : 'standby';
}

export const LAMP: Record<LampState, { chip: Chip; filled: boolean }> = {
  'on-air': { chip: CHIP.onAir, filled: true },
  auto: { chip: CHIP.auto, filled: false },
  'playout-down': { chip: CHIP.playoutDown, filled: false },
  standby: { chip: CHIP.standby, filled: false },
  fault: { chip: CHIP.fault, filled: false },
};

/** A source's switches, as the desk reports them. */
export interface SourceFacts {
  on_air: boolean;
  mute: boolean;
  cue: boolean;
  /** A caller sent to the desk and not yet taken. */
  ready?: boolean;
}

/** Whether a source is open: the room hears it (and listeners, while playout is up). */
export function isOpen(s: SourceFacts): boolean {
  return s.on_air && !s.mute;
}

/** The one word a source's tile shows in its head. */
export function sourceChip(s: SourceFacts, playoutConnected: boolean, mode?: StationMode | null): Chip {
  if (s.on_air && s.mute) return CHIP.muted;
  if (s.on_air) return listenersHear(playoutConnected, mode) ? CHIP.onAir : CHIP.open;
  if (s.cue) return CHIP.cue;
  if (s.ready) return CHIP.ready;
  return CHIP.standby;
}

/** A caller in the screener's queue. */
export interface CallerFacts {
  state: 'waiting' | 'screening' | 'ready';
  on_air: boolean;
  cue: boolean;
  present: boolean;
  linked: boolean;
  dropped_ms: number | null;
  declined: { at_ms: number; reason: string } | null;
  banned_before: { date: string; reason: string } | null;
}

/** A caller's chips: the state first, then what changes what the screener does next. */
export function callerChips(c: CallerFacts, playoutConnected: boolean, mode?: StationMode | null): Chip[] {
  const chips: Chip[] = [];
  if (c.dropped_ms !== null) chips.push(CHIP.dropped);
  else if (c.on_air) chips.push(listenersHear(playoutConnected, mode) ? CHIP.onAir : CHIP.open);
  else if (c.cue) chips.push(CHIP.cue);
  else if (c.state === 'screening') chips.push(CHIP.screening);
  else if (c.state === 'ready') chips.push(CHIP.ready);
  else chips.push(CHIP.waiting);
  if (c.declined && c.dropped_ms === null) chips.push(CHIP.deskBusy);
  if (c.dropped_ms === null && c.present && !c.linked) chips.push(CHIP.linkDegraded);
  if (c.banned_before) chips.push(CHIP.bannedBefore);
  return chips;
}
