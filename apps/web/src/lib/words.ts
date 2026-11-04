// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What a join page says: one banner for where the person stands, and one line for the show.
// The first live run failed because nothing on a guest's page said when the presenter was
// live, so every page states, in words and not colour alone, whether they are on air, waiting
// or next, and whether the show itself is on air. Pure, so the rules are tested.
//
// The chips are the state vocabulary's (packages/ui/states.ts): ON AIR only when listeners hear
// it (ADR-0062), OPEN while playout is down (ADR-0079), DESK BUSY (ADR-0069).

import { CHIP, type Chip } from '../../../../packages/ui/states.ts';
import type { MeState, Role } from './api.ts';

/** The page's own link to the room. */
/** `no-mic`: the browser refused the microphone, so the page never joined; it can try again. */
export type Link = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'lost' | 'ended' | 'no-mic';

/** The banner's mood: picks its colour band, never its meaning. */
export type Tone = 'onair' | 'open' | 'cue' | 'talk' | 'wait' | 'good' | 'warn' | 'ended';

export interface Banner {
  tone: Tone;
  chip: Chip;
  headline: string;
  detail: string;
}

const CONNECTING: Chip = { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Connecting' };
const ENDED: Chip = { cls: 'hollow hollow-off', glyph: 'g-muted', glyphCls: '', word: 'Ended' };
const NO_MIC: Chip = { cls: 'hollow hollow-off', glyph: 'g-muted', glyphCls: '', word: 'No microphone' };
const OFF_AIR: Chip = { cls: 'hollow hollow-standby', glyph: 'g-standby', glyphCls: '', word: 'Off air' };

/** "Rory" from "Rory Bruce"; a fallback when there is no name. */
export function firstName(name: string, fallback: string): string {
  return name.trim().split(/\s+/)[0] || fallback;
}

/** `m:ss` for a countdown. */
export function mmss(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** The banner: the link first (a page that is not connected cannot be on air), then the phase. */
export function banner(role: Role, me: MeState | null, link: Link, now: number, heldUntil: number | null): Banner {
  const p = firstName(me?.presenter ?? '', 'the presenter');
  const P = p === 'the presenter' ? 'The presenter' : p;
  if (link === 'ended' || me?.phase === 'ended') {
    return role === 'caller'
      ? { tone: 'ended', chip: ENDED, headline: 'Your call has ended', detail: `Thanks for calling ${me?.station || 'the station'}. To call again, start from the player page.` }
      : { tone: 'ended', chip: ENDED, headline: 'You have left the studio', detail: 'Open your invite link again to rejoin while it lasts.' };
  }
  // Not a lost line: the page never joined (Firefox on a phone whose settings deny it the
  // microphone said "The line was lost", 2026-11-03).
  if (link === 'no-mic') {
    return {
      tone: 'warn',
      chip: NO_MIC,
      headline: 'Your microphone is blocked',
      detail: role === 'caller' ? 'The studio cannot hear you yet. Allow the microphone, then try again.' : 'You have not joined yet. Allow the microphone, then try again.',
    };
  }
  if (link === 'lost') {
    return {
      tone: 'ended',
      chip: ENDED,
      headline: 'The line was lost',
      detail: role === 'caller' ? 'Your place was held for 5 minutes and has gone. To call again, start from the player page.' : 'Check your connection, then open your invite link again.',
    };
  }
  if (link === 'reconnecting') {
    const left = heldUntil !== null ? heldUntil - now : null;
    return {
      tone: 'warn',
      chip: CHIP.linkDegraded,
      headline: 'Reconnecting…',
      detail:
        role === 'caller' && left !== null
          ? `Your place in the queue is held for ${mmss(left)}. Stay on this page.`
          : 'Stay on this page; it reconnects by itself.',
    };
  }
  if (link !== 'connected' || !me || me.phase === 'joining') {
    return { tone: 'wait', chip: CONNECTING, headline: 'Connecting you to the studio…', detail: 'This takes a few seconds. Allow the microphone if your browser asks.' };
  }
  if (me.phase === 'on_air') {
    if (me.muted) return { tone: 'warn', chip: CHIP.muted, headline: 'On air, muted by the studio', detail: 'Listeners cannot hear you just now. The studio will open your microphone when it is time.' };
    if (me.open) return { tone: 'open', chip: CHIP.open, headline: 'The studio hears you', detail: "Listeners don't, just now: the station's stream is down. Carry on; the studio can hear you." };
    return {
      tone: 'onair',
      chip: CHIP.onAir,
      headline: "You're on air",
      detail: role === 'caller' ? 'Everyone listening can hear you. Speak normally, and turn any radio off.' : 'Everyone listening can hear you.',
    };
  }
  if (me.phase === 'cue') {
    return { tone: 'cue', chip: CHIP.cue, headline: `Your turn is coming: ${p} can hear you`, detail: `You're not on air yet. ${P} may say hello first.` };
  }
  if (role === 'caller') {
    switch (me.phase) {
      case 'screening':
        return me.screener_talking
          ? { tone: 'talk', chip: CHIP.talk, headline: 'The producer is talking to you', detail: 'Only the studio hears you. Speak normally.' }
          : { tone: 'talk', chip: CHIP.screening, headline: 'The producer has picked up your call', detail: "They'll speak to you here in a moment. You're not on air." };
      case 'ready':
        return { tone: 'good', chip: CHIP.ready, headline: "You're next", detail: `The producer has passed you to ${p}. Stay on the line; this page says when you're on air.` };
      case 'desk_busy':
        return { tone: 'warn', chip: CHIP.deskBusy, headline: `${P} is busy just now`, detail: "You keep your place. Stay on the line; you'll hear the show while you wait." };
      default: {
        const where = me.place ? `You're number ${me.place} in the queue` : "You're in the queue";
        return me.screener_talking
          ? { tone: 'talk', chip: CHIP.talk, headline: 'The producer is talking to you', detail: 'Only the studio hears you. Speak normally.' }
          : { tone: 'wait', chip: CHIP.waiting, headline: where, detail: "You'll hear the show while you wait. The producer will pick up soon; you're not on air." };
      }
    }
  }
  if (role === 'dj') {
    return { tone: 'wait', chip: OFF_AIR, headline: 'Waiting for the handover', detail: `You hear the show. You go on air when ${p} hands over to you.` };
  }
  return { tone: 'wait', chip: OFF_AIR, headline: "You're off air", detail: "Listeners can't hear you. You hear the show, and the studio can bring you back on." };
}

/** The line about the show itself, so nobody has to guess whether the presenter is live. */
export function showLine(me: Pick<MeState, 'presenter' | 'presenter_live' | 'show_on_air' | 'desk_reachable'> | null): string {
  if (!me) return '';
  if (!me.desk_reachable) return "The studio can't be reached just now.";
  const p = me.presenter.trim();
  if (me.show_on_air) return me.presenter_live && p ? `The show is on air · ${p} is live` : 'The show is on air';
  if (me.presenter_live) return `${p || 'The presenter'} is live in the studio, but listeners can't hear the show just now`;
  return 'The show is off air just now';
}
