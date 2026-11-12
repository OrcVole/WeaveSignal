// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { ApiError, type MeState, type Phase, type Role } from './lib/api.ts';
import { banner, mmss, showLine } from './lib/words.ts';
import { CLOSED_WHILE_FILLING, LINES_CLOSED, closedLine, closedUnderForm, releasedWords } from './lib/lines.ts';

function me(over: Partial<MeState> = {}): MeState {
  return {
    identity: 'caller-1', role: 'caller', phase: 'waiting', open: false, muted: false, place: 2, queue: 3,
    screener_talking: false, talkback: false, held_until_ms: null, station: 'Kettle FM', show: 'The Morning Kettle',
    presenter: 'Rory Bruce', presenter_live: true, show_on_air: true, desk_reachable: true, ...over,
  };
}
const say = (role: Role, m: Partial<MeState>, link: Parameters<typeof banner>[2] = 'connected', held: number | null = null) =>
  banner(role, me({ role, ...m }), link, 0, held);

describe('what a caller’s page says', () => {
  it('waiting, with the place in the queue, and never "on air"', () => {
    const b = say('caller', {});
    expect(b.headline).toBe("You're number 2 in the queue");
    expect(b.chip.word).toBe('Waiting');
    expect(b.detail).toMatch(/not on air/);
  });
  it('the producer talking while held T, then picked up, ready, busy, cued, on air', () => {
    expect(say('caller', { phase: 'waiting', screener_talking: true }).headline).toBe('The producer is talking to you');
    // At the Callers step there is no screener: the presenter answers, and the page names them.
    expect(say('caller', { phase: 'waiting', screener_talking: true, presenter_answers: true }).headline).toBe('Rory is talking to you');
    expect(say('caller', { phase: 'waiting', place: 1, presenter_answers: true }).detail).toMatch(/Rory will pick up soon/);
    expect(say('caller', { phase: 'screening', presenter_answers: true }).headline).toBe('Rory has picked up your call');
    expect(say('caller', { phase: 'ready', presenter_answers: true }).detail).not.toMatch(/producer/);
    expect(say('caller', { phase: 'screening' }).chip.word).toBe('Screening');
    expect(say('caller', { phase: 'screening', screener_talking: true }).tone).toBe('talk');
    expect(say('caller', { phase: 'ready' }).headline).toBe("You're next");
    expect(say('caller', { phase: 'desk_busy' }).headline).toBe('Rory is busy just now');
    expect(say('caller', { phase: 'desk_busy' }).chip.word).toBe('Desk busy');
    expect(say('caller', { phase: 'cue' }).headline).toBe('Your turn is coming: Rory can hear you');
    const on = say('caller', { phase: 'on_air' });
    expect([on.headline, on.chip.word, on.tone]).toEqual(["You're on air", 'On air', 'onair']);
  });
  it('on air while playout is down is OPEN, not ON AIR (ADR-0062)', () => {
    const b = say('caller', { phase: 'on_air', open: true });
    expect(b.chip.word).toBe('Open');
    expect(b.headline).toBe('The studio hears you');
  });
  it('muted on air says so', () => {
    expect(say('guest', { phase: 'on_air', muted: true }).chip.word).toBe('Muted');
  });
  it('a dropped link counts down the held place', () => {
    const b = say('caller', {}, 'reconnecting', 272_000);
    expect(b.headline).toBe('Reconnecting…');
    expect(b.detail).toMatch(/held for 4:32/);
    expect(say('caller', {}, 'lost').headline).toBe('The line was lost');
  });
  it('a refused microphone is not a lost line: the page never joined and can try again', () => {
    for (const role of ['caller', 'guest'] as const) {
      const b = say(role, {}, 'no-mic');
      expect(b.headline).toBe('Your microphone is blocked');
      expect(b.chip.word).toBe('No microphone');
      expect(b.detail).toMatch(/try again/);
      expect(b.detail).not.toMatch(/connection|held|gone/);
    }
  });
  it('the call ending, from the studio or the caller', () => {
    expect(say('caller', { phase: 'ended' }).headline).toBe('Your call has ended');
    expect(say('caller', {}, 'ended').chip.word).toBe('Ended');
  });
  it('before the desk has seen them: connecting, never a state they are not in', () => {
    for (const p of ['joining'] as Phase[]) expect(say('caller', { phase: p }).headline).toMatch(/Connecting/);
    expect(banner('caller', null, 'connecting', 0, null).chip.word).toBe('Connecting');
    expect(say('caller', { phase: 'on_air' }, 'connecting').chip.word).toBe('Connecting');
  });
});

describe('what a guest’s, a DJ’s and a presenter’s page says', () => {
  it('a guest or presenter on air, off air', () => {
    expect(say('guest', { phase: 'on_air' }).headline).toBe("You're on air");
    expect(say('presenter', { phase: 'on_air' }).chip.word).toBe('On air');
    expect(say('guest', { phase: 'off_air' }).headline).toBe("You're off air");
  });
  it('a remote DJ waits for the handover', () => {
    const b = say('dj', { phase: 'off_air' });
    expect(b.headline).toBe('Waiting for the handover');
    expect(b.detail).toMatch(/Rory hands over/);
  });
  it('a guest leaving is not told about a queue', () => {
    expect(say('guest', {}, 'ended').headline).toBe('You have left the studio');
  });
});

describe('the show line says whether the presenter is live', () => {
  it('on air and live', () => expect(showLine(me())).toBe('The show is on air · Rory Bruce is live'));
  it('live in the studio while listeners hear nothing', () =>
    expect(showLine(me({ show_on_air: false }))).toMatch(/Rory Bruce is live in the studio, but listeners can't hear/));
  it('off air', () => expect(showLine(me({ show_on_air: false, presenter_live: false }))).toBe('The show is off air just now'));
  it('the studio unreachable', () => expect(showLine(me({ desk_reachable: false }))).toMatch(/can't be reached/));
  it('m:ss', () => expect([mmss(0), mmss(59_001), mmss(300_000)]).toEqual(['0:00', '1:00', '5:00']));
});

describe('a caller let go because the lines closed (ADR-0271)', () => {
  const gone = (released: MeState['released'], link: Parameters<typeof banner>[2], name = 'Niall Anderson') =>
    banner('caller', me({ phase: 'ended', released }), link, 0, null, name);

  it('is thanked by name and told they were not on air, with the cause', () => {
    const took = gone('show_took_air', 'ended');
    expect(took.headline).toBe('The lines have closed');
    expect(took.detail).toBe('Thank you for waiting, Niall. The show that is starting now is not taking calls, so your place in the queue has gone. You were not on air.');
    expect(took.chip.word).toBe('Lines closed');
    expect(took.tone).toBe('ended');
    const down = gone('step_moved_down', 'ended');
    expect(down.detail).toBe('Thank you for waiting, Niall. This show has stopped taking calls, so your place in the queue has gone. You were not on air.');
  });

  it('reads so whatever the link is doing: never "Your call has ended" or "The line was lost"', () => {
    for (const link of ['ended', 'lost', 'reconnecting', 'connected', 'connecting'] as const) {
      expect(gone('step_moved_down', link).headline, link).toBe('The lines have closed');
    }
    // Without the cause it is an ordinary end.
    expect(gone(null, 'ended').headline).toBe('Your call has ended');
    expect(banner('caller', me({ phase: 'waiting' }), 'lost', 0, null, 'Niall Anderson').headline).toBe('The line was lost');
  });

  it('thanks without a name when none is known, and is a caller’s banner only', () => {
    expect(gone('show_took_air', 'ended', '').detail).toMatch(/^Thank you for waiting\. The show that is starting now/);
    expect(banner('guest', me({ role: 'guest', phase: 'ended', released: 'step_moved_down' }), 'ended', 0, null, 'Gregor Stewart').headline).toBe('You have left the studio');
  });
});

describe('the lines closed for a show (ADR-0271)', () => {
  it('says the station’s sentence, and adds the chat while it is open to write in', () => {
    expect(closedLine(LINES_CLOSED, false)).toBe('The lines are closed for this show.');
    expect(closedLine(LINES_CLOSED, true)).toBe('The lines are closed for this show. You can still write in the chat below.');
    // Another reason for no calls is the station's own, and nothing is added to it.
    const other = 'The studio is not taking calls right now. Keep listening and try again later.';
    expect(closedLine(other, true)).toBe(other);
    expect(closedLine(null, true)).toBe('');
  });

  it('tells a form sent after the lines closed from any other refusal, by the station’s sentence', () => {
    expect(closedUnderForm(new ApiError(503, LINES_CLOSED))).toBe(true);
    expect(closedUnderForm(new ApiError(503, 'Calls are not set up at this station yet.'))).toBe(false);
    expect(closedUnderForm(new ApiError(422, 'Your name is too long', 'name'))).toBe(false);
    expect(closedUnderForm(new TypeError('Failed to fetch'))).toBe(false);
    expect(CLOSED_WHILE_FILLING).toBe('The lines closed while you were filling this in. This show is not taking calls.');
  });

  it('words a released caller’s page for each cause', () => {
    expect(releasedWords('show_took_air', 'Niall Anderson')).toEqual({
      headline: 'The lines have closed',
      thanks: 'Thank you for waiting, Niall.',
      detail: 'The show that is starting now is not taking calls, so your place in the queue has gone. You were not on air.',
    });
    expect(releasedWords('step_moved_down', '  ').thanks).toBe('Thank you for waiting.');
  });
});

describe('invitations', () => {
  it('are the noun on every join page', () => {
    expect(banner('guest', me({ role: 'guest', phase: 'ended' }), 'ended', 0, null).detail).toBe('Open your invitation link again to rejoin while it lasts.');
    expect(banner('guest', me({ role: 'guest' }), 'lost', 0, null).detail).toBe('Check your connection, then open your invitation link again.');
  });
});
