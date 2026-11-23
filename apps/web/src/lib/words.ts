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
import type { MeState, Role, Studio } from './api.ts';
import { releasedWords } from './lines.ts';
import type { Moved } from './move.ts';

/** The page's own link to the room. */
/** `no-mic`: the browser refused the microphone, so the page never joined; it can try again.
 *  `moving`: the rehearsal studio moved the person, and the page is joining the live studio. */
export type Link = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'lost' | 'ended' | 'no-mic' | 'moving';

/** The banner's mood: picks its colour band, never its meaning. `rehearsal` is the rehearsal
 *  studio's own band: never the red of ON AIR, never the white of OPEN (ADR-0062). */
export type Tone = 'onair' | 'open' | 'cue' | 'talk' | 'wait' | 'good' | 'warn' | 'ended' | 'rehearsal';

/** What the page knows beyond the person's own state: where they are, whether they have been
 *  on air since they joined (`off_air` alone cannot tell "standing by" from "taken off"), and
 *  the rehearsal they were moved from, if they were. */
export interface Context {
  studio?: Studio;
  wasOnAir?: boolean;
  moved?: Moved | null;
}

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
// The rehearsal studio's own lamp: hollow with a double rule, never red or white. The glyph is
// the repeat sign until the design has one of its own (the studio's REHEARSAL lamp, screen 2).
const REHEARSAL: Chip = { cls: 'hollow hollow-rehearsal', glyph: 'g-repeat', glyphCls: '', word: 'Rehearsal' };
const MOVING: Chip = { cls: 'hollow hollow-rehearsal', glyph: 'g-moved', glyphCls: '', word: 'Moving' };
const LINES_CLOSED: Chip = { cls: 'hollow hollow-off', glyph: 'g-muted', glyphCls: '', word: 'Lines closed' };

/** "Rory" from "Rory Bruce"; a fallback when there is no name. */
export function firstName(name: string, fallback: string): string {
  return name.trim().split(/\s+/)[0] || fallback;
}

/** `m:ss` for a countdown. */
export function mmss(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** The banner: the link first (a page that is not connected cannot be on air), then the phase.
 *  `name` is the person's own, as they gave it, for the one banner that thanks them. */
export function banner(role: Role, me: MeState | null, link: Link, now: number, heldUntil: number | null, name = '', ctx: Context = {}): Banner {
  const rehearsal = (ctx.studio ?? me?.studio) === 'rehearsal';
  const p = firstName(me?.presenter ?? '', 'the presenter');
  const P = p === 'the presenter' ? 'The presenter' : p;
  // Who answers the phone: the producer, or at the Callers step the presenter themselves.
  const A = me?.presenter_answers ? P : 'The producer';
  // A waiting caller let go because the lines closed (ADR-0271): before anything the link says,
  // because their pass is ended and they are taken out of the room at the same moment, which
  // would otherwise read as a call that ended or a line that was lost.
  if (role === 'caller' && me?.released) {
    const w = releasedWords(me.released, name);
    return { tone: 'ended', chip: LINES_CLOSED, headline: w.headline, detail: `${w.thanks} ${w.detail}` };
  }
  if (link === 'ended' || me?.phase === 'ended') {
    if (rehearsal) {
      return { tone: 'ended', chip: ENDED, headline: 'You have left the rehearsal studio', detail: 'Open your invitation link again to come back while the rehearsal is open.' };
    }
    return role === 'caller'
      ? { tone: 'ended', chip: ENDED, headline: 'Your call has ended', detail: `Thanks for calling ${me?.station || 'the station'}. To call again, start from the player page.` }
      : { tone: 'ended', chip: ENDED, headline: 'You have left the studio', detail: 'Open your invitation link again to rejoin while it lasts.' };
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
      detail: role === 'caller' ? 'Your place was held for 5 minutes and has gone. To call again, start from the player page.' : 'Check your connection, then open your invitation link again.',
    };
  }
  // The presenter moved everyone: the page is on its way to the live studio, microphone kept.
  if (link === 'moving') {
    return {
      tone: 'rehearsal',
      chip: MOVING,
      headline: 'Moving you to the live studio…',
      detail: 'Nobody goes on air until the handover. Stay on this page; it takes a few seconds.',
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
    return {
      tone: 'wait',
      chip: CONNECTING,
      headline: rehearsal ? 'Connecting you to the rehearsal studio…' : 'Connecting you to the studio…',
      detail: 'This takes a few seconds. Allow the microphone if your browser asks.',
    };
  }
  // The rehearsal studio reaches no air, so none of the live words below is true there: `on_air`
  // is only "in the rehearsal's mix", and nobody is ever on air or open (ADR-0062).
  if (rehearsal) return rehearsalBanner(me);
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
          ? { tone: 'talk', chip: CHIP.talk, headline: `${A} is talking to you`, detail: 'Only the studio hears you. Speak normally.' }
          : { tone: 'talk', chip: CHIP.screening, headline: `${A} has picked up your call`, detail: "They'll speak to you here in a moment. You're not on air." };
      case 'ready':
        return { tone: 'good', chip: CHIP.ready, headline: "You're next", detail: me.presenter_answers ? "Stay on the line; this page says when you're on air." : `The producer has passed you to ${p}. Stay on the line; this page says when you're on air.` };
      case 'desk_busy':
        return { tone: 'warn', chip: CHIP.deskBusy, headline: `${P} is busy just now`, detail: "You keep your place. Stay on the line; you'll hear the show while you wait." };
      default: {
        const where = me.place ? `You're number ${me.place} in the queue` : "You're in the queue";
        return me.screener_talking
          ? { tone: 'talk', chip: CHIP.talk, headline: `${A} is talking to you`, detail: 'Only the studio hears you. Speak normally.' }
          : { tone: 'wait', chip: CHIP.waiting, headline: where, detail: `You'll hear the show while you wait. ${A} will pick up soon; you're not on air.` };
      }
    }
  }
  // Every invitation arrives standing by (1.3): `off_air` before the person has been on air
  // since they joined is that, and the words say so. After they were on air it is "taken off".
  const standing = ctx.wasOnAir !== true;
  const from = ctx.moved ? `You moved here from the rehearsal for ${ctx.moved.show}, which ${standbyAiring(ctx.moved.airing_start_ms, now)}. ` : '';
  if (role === 'dj') {
    return { tone: 'wait', chip: CHIP.standby, headline: 'Waiting for the handover', detail: `${from}You hear the show. You go on air when ${p} hands over to you.` };
  }
  if (standing && (role === 'guest' || role === 'presenter')) {
    const until = role === 'guest' && !ctx.moved ? 'until the studio puts you on' : 'until the handover';
    return { tone: 'wait', chip: CHIP.standby, headline: 'Standing by', detail: `${from}You are not on air ${until}. You hear the show, and this page tells you when you are.` };
  }
  return { tone: 'wait', chip: OFF_AIR, headline: "You're off air", detail: "Listeners can't hear you. You hear the show, and the studio can bring you back on." };
}

/** "is due on air at 14:05" or, once past, "was due on air at 14:05". */
function standbyAiring(startMs: number, now: number): string {
  return `${startMs > now ? 'is' : 'was'} due on air at ${clock(startMs)}`;
}

/** A time of day in the person's own zone, as the join page's other times are. */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/** What the rehearsal studio's page says once joined. Only the people in the room hear them;
 *  the desk has no way to the air. `on_air` is "in the mix", never an air state. */
function rehearsalBanner(me: MeState): Banner {
  const only = 'Only the people here hear you. Nobody listening to the station can.';
  if (me.phase === 'on_air' && me.muted) {
    return { tone: 'rehearsal', chip: CHIP.muted, headline: 'You are muted in the rehearsal', detail: `${only} The studio will open your microphone when it is time.` };
  }
  if (me.phase === 'on_air') {
    return { tone: 'rehearsal', chip: REHEARSAL, headline: 'You are in the rehearsal studio', detail: `${only} You are in the mix.` };
  }
  return { tone: 'rehearsal', chip: REHEARSAL, headline: 'You are in the rehearsal studio', detail: `${only} You are not in the mix yet; the studio brings you in.` };
}

/** The line about the show itself, so nobody has to guess whether the presenter is live. */
export function showLine(me: Pick<MeState, 'presenter' | 'presenter_live' | 'show_on_air' | 'desk_reachable' | 'show'> & { studio?: Studio } | null): string {
  if (!me) return '';
  if (me.studio === 'rehearsal') {
    return me.desk_reachable ? `Rehearsing ${me.show || 'the show'}. Nobody listening hears this.` : "The rehearsal studio can't be reached just now.";
  }
  if (!me.desk_reachable) return "The studio can't be reached just now.";
  const p = me.presenter.trim();
  if (me.show_on_air) return me.presenter_live && p ? `The show is on air · ${p} is live` : 'The show is on air';
  if (me.presenter_live) return `${p || 'The presenter'} is live in the studio, but listeners can't hear the show just now`;
  return 'The show is off air just now';
}
