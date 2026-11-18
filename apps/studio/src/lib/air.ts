// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Ending the show and handing the air to automation (ADR-0045, ADR-0183, ADR-0272): what each
// control asks before it acts, and what it says after, worked out from the view alone so the
// dialog, the button, the key and the tests agree.
//
// Three asks, each confirmed in the desk's one air dialog (keymap rule 5: ⏎ confirms, Esc
// keeps going):
//   end   End show (⌃⇧E): everyone off air, the carts faded, then the air to automation once
//         the profanity delay has played out.
//   auto  Hand to automation: the mode to auto, now; nobody is taken off air, the desk is just
//         not heard.
//   live  Take the air back: the mode to live, now; whatever is open at the desk is heard.

import type { EndShowResult, Source, View } from './api.ts';
import { firstName } from './format.ts';
import { endsTo } from './post.ts';

export type AirAsk = 'end' | 'auto' | 'live';

/** What the dialog says and offers for an ask. */
export interface AirPlan {
  title: string;
  /** One sentence for the consequence that matters most. */
  lede: string;
  /** What else happens, one clause each. */
  lines: string[];
  confirm: string;
  /** The word on the keeping-going button. */
  keep: string;
  /** What automation is, where the ask gives it the air; null where it does not. */
  about: string | null;
}

/**
 * What automation is and what it plays, said once for every place that names it: the AUTO lamp's
 * tooltip, the tooltip of the button under the lamp, and the air dialog (the operator, 2026-11-17:
 * "what is the automation? The automatic what?"). It follows playout's own order (the product's
 * Playout reference, "What is on air"): the log, then the station's fallback files, then the loop
 * WeaveSignal ships. Idents, stingers and adverts are examples of the carts a station puts in its
 * clocks, not kinds the log knows.
 */
export const AUTOMATION_PLAYS =
  'Automation is the station playing by itself. It plays the log: the music planned for this hour from the station’s library, with carts such as idents, stingers or adverts between the tracks. With nothing in the log, listeners hear the station’s fallback files, or the loop WeaveSignal ships.';

/** The people the room hears: on air and not muted (the desk's own "open"). */
function openSources(v: View): Source[] {
  return v.sources.filter((s) => s.on_air && !s.mute);
}

/** Where those taken off air are left: in the room, or, for a host's own studio (ADR-0280),
 *  standing by. */
function stays(list: Source[]): string {
  const own = list.filter((s) => s.role === 'source').length;
  return own === 0 ? 'in the room' : own === list.length ? 'standing by' : 'in the room or standing by';
}

function names(list: Source[]): string {
  const n = list.map((s) => firstName(s.name, s.identity));
  if (n.length <= 1) return n.join('');
  return `${n.slice(0, -1).join(', ')} and ${n[n.length - 1]}`;
}

const secs = (ms: number) => Math.max(1, Math.round(ms / 1000));

/**
 * Why the mode control cannot act, or null. The station's mode is a playout setting, so a station
 * without the playout layer (`mode` null, or absent from an old control plane) has no automation
 * to hand the air to. The desk being away does not stop it: handing the air to automation is what
 * to do when the desk is down.
 */
export function modeRefusal(v: View | null): string | null {
  if (!v) return 'Waiting for the station.';
  if (v.mode === null || v.mode === undefined) {
    return 'This station has no playout layer, so there is no automation to hand the air to. Automation needs the database and playout.';
  }
  return null;
}

/** What the button says it does, first in its tooltip: who has the air after, and that nobody is
 *  cut off. What automation is (`AUTOMATION_PLAYS`) follows it there. */
export const MODE_WHY = {
  auto: 'Automation takes the air now. Whoever is open stays in the room.',
  live: 'The air comes back to the desk. Whoever is open reaches listeners.',
} as const;

/** What the button under the lamp does next, from the mode. */
export function modeButton(v: View | null): { ask: AirAsk; label: string } {
  return v?.mode === 'auto'
    ? { ask: 'live', label: 'Take the air back' }
    : { ask: 'auto', label: 'Hand to automation' };
}

/** Whether the button can act at once, with nothing to confirm: nobody is open, so nobody is
 *  cut off (to auto) or put on the air (to live). */
export function needsConfirm(ask: AirAsk, v: View): boolean {
  return ask === 'end' || openSources(v).length > 0;
}

export function airPlan(ask: AirAsk, v: View): AirPlan {
  const open = openSources(v);
  const onAir = v.sources.filter((s) => s.on_air);
  const delay = v.desk.delay;
  const held = delay && delay.delay_ms >= 500 ? secs(delay.delay_ms) : 0;
  if (ask === 'end') {
    const lines: string[] = [];
    // The next host is standing by (ADR-0280): the air passes to them, and nothing goes to
    // automation. The dialog says which before the press, as the button's tooltip does.
    const next = endsTo(v);
    lines.push(
      onAir.length
        ? `${names(onAir)} ${onAir.length === 1 ? 'goes' : 'go'} off air now. They stay ${stays(onAir)}, and callers stay in the queue.`
        : 'Nobody is on air.',
    );
    if (v.carts.sounding.length) lines.push('Every cart that is sounding fades.');
    if (next) {
      lines.push(`End show passes the air to ${next.name}, at once. Nothing goes to automation.`);
      if (!v.desk.reachable) lines.unshift('The desk is not answering, so nobody can be taken off air from here.');
      return {
        title: 'End the show?',
        // During a grace the show's name on the desk is already the next show's (ADR-0214), so
        // the lede names who takes over, not what ends.
        lede: `${next.name} is standing by: ${next.show} starts as you end.`,
        lines,
        confirm: 'End show',
        keep: 'Keep going',
        about: null,
      };
    }
    if (v.mode === null || v.mode === undefined) {
      lines.push('This station has no automation to hand the air to (it needs the database and playout), so that is all that happens.');
    } else if (v.mode === 'auto') {
      lines.push('Automation already holds the air.');
    } else {
      lines.push(
        held
          ? `Listeners hear the last ${held} s of the show, which the profanity delay still holds, and then automation takes the air.`
          : 'Automation takes the air.',
      );
    }
    if (!v.desk.reachable) lines.unshift('The desk is not answering, so nobody can be taken off air from here.');
    return {
      title: 'End the show?',
      lede: v.show.title ? `${v.show.title} ends for listeners.` : 'The show ends for listeners.',
      lines,
      confirm: 'End show',
      keep: 'Keep going',
      // A station with no playout layer has no automation to explain.
      about: v.mode === null || v.mode === undefined ? null : AUTOMATION_PLAYS,
    };
  }
  if (ask === 'auto') {
    return {
      title: 'Hand the air to automation?',
      lede: open.length
        ? `Listeners stop hearing ${names(open)} at once, without waiting for the profanity delay.`
        : 'Nobody is open at the desk, so nobody is cut off.',
      lines: [
        'Nobody is taken off air: whoever is open stays open in the room, and says OPEN, not ON AIR.',
        'To end the show properly, with the delay played out and the carts faded, use End show instead.',
      ],
      confirm: 'Hand to automation',
      keep: 'Keep the air',
      about: AUTOMATION_PLAYS,
    };
  }
  return {
    title: 'Take the air back?',
    lede: open.length
      ? `${names(open)} ${open.length === 1 ? 'is' : 'are'} open at the desk and reach${open.length === 1 ? 'es' : ''} listeners the moment you confirm.`
      : 'Nobody is open at the desk, so listeners hear the station until someone is put on air.',
    lines: ['Automation still fills any silence if the desk goes quiet.'],
    confirm: 'Take the air back',
    keep: 'Leave it on automation',
    about: null,
  };
}

/** The toast after End show. */
export function endShowNote(r: EndShowResult): { title: string; message: string } {
  const off =
    r.off_air === 0
      ? 'Nobody was on air.'
      : `${r.off_air} ${r.off_air === 1 ? 'source is' : 'sources are'} off air.`;
  // The air passed to the next host (ADR-0280): said first, because nothing went to automation
  // and `automation_in_ms` is null then, as it is on a station with no playout layer.
  if (r.passed_to) {
    const who = firstName(r.passed_to, r.passed_to);
    return { title: `${who} has the air`, message: `${off} The air passed to ${who}; nothing went to automation.` };
  }
  if (r.automation_in_ms === null) {
    return {
      title: 'The show is over',
      message: `${off} The carts are fading. This station has no automation to hand the air to.`,
    };
  }
  if (r.automation_in_ms === 0) {
    return { title: 'The show is over', message: `${off} Automation already holds the air.` };
  }
  return {
    title: 'The show is over',
    message: `${off} Automation takes the air in ${secs(r.automation_in_ms)} s, when the last of the delay has played out.`,
  };
}

/** The toast after a mode change. */
export function modeNote(ask: 'auto' | 'live'): { title: string; message: string } {
  return ask === 'auto'
    ? { title: 'Automation holds the air', message: 'Listeners hear the log. Open faders say OPEN until you take the air back.' }
    : { title: 'You have the air', message: 'The desk reaches listeners again: open faders say ON AIR.' };
}
