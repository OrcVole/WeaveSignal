// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The join page in the rehearsal studio and on the move to the live studio (1.3): the words
// (nothing there is on air or open, ADR-0062), the standing-by arrival, the fragment swap,
// and the move itself with a fake room: the microphone kept, the stream following the new pass.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { LocalAudioTrack } from 'livekit-client';
import type { Joined, MeState, MoveTo, Role } from './lib/api.ts';
import { AS, caps, heading, joinWord, lasts, movedHere, what } from './lib/join-words.ts';
import { MOVE_RETRY_MS, adoptPass } from './lib/move.ts';
import { banner, clock, showLine } from './lib/words.ts';

// What the call does, in order: the room, the stream, the fragment.
const events: string[] = [];
const followed: { pass: string; onState: (s: MeState) => void }[] = [];
let tokens: (Joined | Error)[] = [];
let mic: LocalAudioTrack & { stop: ReturnType<typeof vi.fn> };

vi.mock('livekit-client', () => {
  class Room {
    remoteParticipants = new Map();
    canPlaybackAudio = true;
    localParticipant = {
      setTrackSubscriptionPermissions: () => {},
      publishTrack: async (t: unknown, o: { source: string }) => void events.push(`publish ${o.source} ${t === mic ? 'same track' : 'other track'}`),
    };
    on() {
      return this;
    }
    async connect(url: string) {
      events.push(`connect ${url}`);
    }
    async disconnect(stopTracks?: boolean) {
      events.push(`disconnect stopTracks=${stopTracks}`);
    }
  }
  return {
    Room,
    RoomEvent: {},
    DisconnectReason: {},
    Track: { Source: { Camera: 'camera', Microphone: 'microphone' }, Kind: {} },
    createLocalAudioTrack: async () => {
      events.push('capture microphone');
      return mic;
    },
    createLocalVideoTrack: async () => {
      events.push('capture camera');
      return { mediaStreamTrack: {} as MediaStreamTrack, stop: () => {}, mute: async () => {}, unmute: async () => {} };
    },
  };
});

vi.mock('./lib/api.ts', async (orig) => ({
  ...(await orig<typeof import('./lib/api.ts')>()),
  followMe: (pass: string, onState: (s: MeState) => void) => {
    events.push(`follow ${pass}`);
    followed.push({ pass, onState });
    return () => events.push(`stop following ${pass}`);
  },
  passToken: async (pass: string) => {
    events.push(`token ${pass}`);
    const j = tokens.shift();
    if (!j) throw new Error('no token');
    if (j instanceof Error) throw j;
    return j;
  },
}));

const rehearsalJoined = (see: string | null = null): Joined => ({
  pass: 'rsp-rehearsal', token: 't1', url: 'ws://rehearsal', room: 'studio-rehearsal', identity: 'ina', role: 'guest', name: 'Kirsty Wilson', hear: 'desk-rehearsal-mm-ina', see, pass_expires_at_ms: Date.now() + 3600_000,
});
const liveJoined = (): Joined => ({ ...rehearsalJoined('desk-pgm'), pass: 'rsp-live', token: 't2', url: 'ws://live', room: 'studio', hear: 'desk-mm-ina', see: 'desk-pgm' });

const MOVE: MoveTo = { pass: 'rsp-live', link: 'https://k.example/join/#rsp-live', show: 'Steeped', airing_start_ms: Date.now() + 600_000 };
function me(over: Partial<MeState> = {}): MeState {
  return {
    identity: 'ina', role: 'guest', phase: 'on_air', open: false, muted: false, place: null, queue: 0, screener_talking: false, talkback: false, held_until_ms: null,
    station: 'Kettle FM', show: 'Steeped', presenter: '', presenter_live: false, show_on_air: false, desk_reachable: true, studio: 'rehearsal', ...over,
  };
}
const say = (role: Role, m: Partial<MeState>, link: Parameters<typeof banner>[2] = 'connected', ctx: Parameters<typeof banner>[6] = {}) =>
  banner(role, me({ role, ...m }), link, 0, null, '', ctx);

beforeEach(() => {
  events.length = 0;
  followed.length = 0;
  tokens = [];
  mic = { mediaStreamTrack: { readyState: 'live' } as MediaStreamTrack, stop: vi.fn() } as unknown as typeof mic;
});

describe('what the rehearsal studio’s page says', () => {
  it('says where they are and who hears them, and is never ON AIR or OPEN', () => {
    for (const phase of ['on_air', 'off_air'] as const) {
      const b = say('guest', { phase });
      expect(b.headline).toBe('You are in the rehearsal studio');
      expect(b.detail).toMatch(/^Only the people here hear you\./);
      expect([b.chip.word, b.tone]).toEqual(['Rehearsal', 'rehearsal']);
      expect(b.chip.cls).not.toMatch(/onair|open|critical/);
    }
    expect(say('guest', { phase: 'on_air' }).detail).toMatch(/You are in the mix\.$/);
    expect(say('guest', { phase: 'off_air' }).detail).toMatch(/not in the mix yet/);
  });

  it('never turns the rehearsal’s mix into an air state, whatever the desk reports', () => {
    const odd = say('presenter', { phase: 'on_air', open: true, show_on_air: true, presenter_live: true });
    expect(odd.tone).toBe('rehearsal');
    expect(`${odd.headline} ${odd.detail} ${odd.chip.word}`).not.toMatch(/on air|The studio hears you|Listeners/i);
    expect(say('guest', { phase: 'on_air', muted: true }).headline).toBe('You are muted in the rehearsal');
  });

  it('a link that has not yet joined says the rehearsal studio, and a move says where to', () => {
    expect(say('guest', { phase: 'joining' }, 'connecting').headline).toBe('Connecting you to the rehearsal studio…');
    const m = say('guest', {}, 'moving');
    expect([m.headline, m.chip.word, m.tone]).toEqual(['Moving you to the live studio…', 'Moving', 'rehearsal']);
    expect(m.detail).toMatch(/Nobody goes on air until the handover/);
    // A reconnecting or lost line keeps the usual words.
    expect(say('guest', {}, 'reconnecting').headline).toBe('Reconnecting…');
    expect(say('guest', {}, 'ended').headline).toBe('You have left the rehearsal studio');
  });

  it('the show line says what is being rehearsed, not that the show is off air', () => {
    expect(showLine(me())).toBe('Rehearsing Steeped. Nobody listening hears this.');
    expect(showLine(me({ desk_reachable: false }))).toBe("The rehearsal studio can't be reached just now.");
  });
});

describe('arriving at the live studio standing by', () => {
  const live = (role: Role, ctx: Parameters<typeof banner>[6] = {}) => say(role, { phase: 'off_air', studio: 'live', presenter: 'Eilidh Fraser-MacLeod' }, 'connected', ctx);

  it('a guest and a presenter stand by, and say until what', () => {
    expect(live('guest').headline).toBe('Standing by');
    expect(live('guest').detail).toMatch(/You are not on air until the studio puts you on\./);
    expect(live('presenter').detail).toMatch(/You are not on air until the handover\./);
  });

  it('a person moved from the rehearsal is told where from and when the show is due', () => {
    const soon = { show: 'Steeped', airing_start_ms: 1_000_000 };
    const at = (moved: typeof soon, now: number) => banner('guest', me({ phase: 'off_air', studio: 'live' }), 'connected', now, null, '', { moved }).detail;
    expect(at(soon, 2_000_000)).toBe(`You moved here from the rehearsal for Steeped, which was due on air at ${clock(1_000_000)}. You are not on air until the handover. You hear the show, and this page tells you when you are.`);
    expect(at(soon, 0)).toContain(`which is due on air at ${clock(1_000_000)}.`);
    expect(live('dj', { moved: soon }).detail).toMatch(/^You moved here from the rehearsal for Steeped.* You go on air when Eilidh hands over to you\.$/);
  });

  it('once they have been on air, off air is a studio taking them off', () => {
    expect(live('guest', { wasOnAir: true }).headline).toBe("You're off air");
  });

  it('no page promises the air at once on joining', () => {
    for (const studio of ['live', 'rehearsal'] as const)
      for (const role of ['guest', 'presenter', 'dj'] as const) expect(what(role, studio)).not.toMatch(/as soon as you join|on air the moment|at once/i);
    expect(what('guest', 'live')).toMatch(/you are not on air until the studio puts you on/);
    expect(what('presenter', 'live')).toMatch(/not on air until the handover/);
  });
});

describe('the join card', () => {
  it('names the rehearsal, who hears it, and the move', () => {
    expect(heading('Steeped', 'presenter', 'rehearsal')).toBe('Rehearse Steeped to present');
    expect(heading('The Morning Kettle', 'guest', 'live')).toBe('Join The Morning Kettle as a guest');
    expect(AS.dj).toBe('as a remote DJ');
    expect([caps('rehearsal'), caps('live')]).toEqual(['Rehearsal invitation', 'Invitation']);
    expect([joinWord('rehearsal'), joinWord('live')]).toEqual(['Join the rehearsal studio', 'Join the studio']);
    expect(what('guest', 'rehearsal')).toBe('This link leads to the rehearsal studio. Only the people in it hear you; nobody listening to the station can. When the presenter moves everyone to the live studio, this page moves you with it, standing by.');
    expect(lasts(0, 'rehearsal')).toContain('while the rehearsal is open');
    expect(lasts(0, 'live')).not.toContain('rehearsal');
  });

  it('says what a page opened after the move took on', () => {
    expect(movedHere({ show: 'Steeped', airing_start_ms: 600_000 }, 0)).toBe(
      `The rehearsal for Steeped has moved to the live studio, which is due on air at ${clock(600_000)}. This page took your place there: you join standing by, and you are not on air until the handover.`,
    );
  });
});

describe('the fragment', () => {
  it('is replaced in place: no new history entry, no assignment to the hash, no reload', () => {
    const replaceState = vi.fn();
    adoptPass({ pathname: '/join/', search: '' }, { state: { k: 1 }, replaceState }, 'rsp-live');
    expect(replaceState).toHaveBeenCalledExactlyOnceWith({ k: 1 }, '', '/join/#rsp-live');
    // Another path or query is kept.
    adoptPass({ pathname: '/join/', search: '?x=1' }, { state: null, replaceState }, 'p2');
    expect(replaceState).toHaveBeenLastCalledWith(null, '', '/join/?x=1#p2');
    // No browser, nothing to do.
    expect(() => adoptPass(undefined, undefined, 'p')).not.toThrow();
  });
});

describe('the move to the live studio', () => {
  async function inRehearsal(opts: { see?: string | null } = {}) {
    const { Call } = await import('./lib/call.svelte.ts');
    const onPass = vi.fn((p: string) => void events.push(`fragment ${p}`));
    const call = new Call(rehearsalJoined(opts.see ?? null), false, null, { studio: 'rehearsal', onPass });
    await call.start();
    return { call, onPass };
  }
  const settle = async (call: { link: string }) => {
    for (let i = 0; i < 100 && call.link !== 'connected'; i++) await new Promise((r) => setTimeout(r, 20));
  };

  it('swaps the fragment first, keeps the microphone, follows the new pass and joins the live room', async () => {
    const { call, onPass } = await inRehearsal();
    expect(call.studio).toBe('rehearsal');
    expect(events).toEqual(['follow rsp-rehearsal', 'capture microphone', 'connect ws://rehearsal', 'publish microphone same track']);
    events.length = 0;
    tokens = [liveJoined()];
    followed[0].onState(me({ move: MOVE }));
    expect(call.link).toBe('moving');
    await settle(call);
    expect(onPass).toHaveBeenCalledExactlyOnceWith('rsp-live');
    expect(events).toEqual([
      'fragment rsp-live', // before anything is dropped: a reload now lands in the live studio
      'stop following rsp-rehearsal',
      'follow rsp-live',
      'disconnect stopTracks=false', // the tracks are kept
      'token rsp-live',
      'capture camera', // the live room offers the picture the rehearsal did not
      'connect ws://live',
      'publish microphone same track', // the same, live microphone: never stopped
      'publish camera other track',
    ]);
    expect(mic.stop).not.toHaveBeenCalled();
    expect(call.link).toBe('connected');
    expect(call.studio).toBe('live');
    expect(call.joined.pass).toBe('rsp-live');
    expect(call.moved).toEqual({ show: 'Steeped', airing_start_ms: MOVE.airing_start_ms });
    expect(call.heldUntil).toBeNull();
  });

  it('moves once: the server sends the move again on every frame', async () => {
    const { call } = await inRehearsal();
    events.length = 0;
    tokens = [liveJoined(), liveJoined()];
    followed[0].onState(me({ move: MOVE }));
    followed[0].onState(me({ move: MOVE, muted: true }));
    followed[0].onState(me({ move: { ...MOVE, pass: 'rsp-live-2' } }));
    await settle(call);
    expect(events.filter((e) => e.startsWith('token'))).toEqual(['token rsp-live']);
    expect(events.filter((e) => e.startsWith('fragment'))).toEqual(['fragment rsp-live']);
    // The old stream's frames after the move are dropped; the new pass's are heard.
    followed[1].onState(me({ studio: 'live', phase: 'off_air', move: null }));
    expect(call.me?.studio).toBe('live');
    followed[0].onState(me({ phase: 'ended' }));
    expect(call.link).toBe('connected');
  });

  it('counts being on air at the live desk, not the rehearsal’s mix', async () => {
    const { call } = await inRehearsal();
    followed[0].onState(me({ phase: 'on_air' }));
    expect(call.wasOnAir).toBe(false);
    tokens = [liveJoined()];
    followed[0].onState(me({ move: MOVE }));
    await settle(call);
    followed[1].onState(me({ studio: 'live', phase: 'off_air' }));
    expect(call.wasOnAir).toBe(false);
    followed[1].onState(me({ studio: 'live', phase: 'on_air' }));
    expect(call.wasOnAir).toBe(true);
  });

  it('says so when the live studio will not take them, not "Moving" for ever', async () => {
    const { ApiError } = await import('./lib/api.ts');
    const { call } = await inRehearsal();
    // A refusal that stays: the show on air's step has switched this seat off (409).
    tokens = [new ApiError(409, 'The show on air has no seat for a guest just now.'), liveJoined()];
    followed[0].onState(me({ move: MOVE }));
    for (let i = 0; i < 100 && !call.problem; i++) await new Promise((r) => setTimeout(r, 20));
    expect(call.problem).toBe('The show on air has no seat for a guest just now.');
    expect(call.link).toBe('moving');
    // It carries on trying, and when it gets in the sentence goes.
    await settle(call);
    expect(call.link).toBe('connected');
    expect(call.problem).toBeNull();
  });

  it('a move pre-empts a rejoin that was spinning on the rehearsal pass', async () => {
    const { ApiError } = await import('./lib/api.ts');
    const { call } = await inRehearsal();
    events.length = 0;
    // The room goes (the server takes people out after the move) before the page has seen the move.
    tokens = [new ApiError(409, 'The rehearsal studio for Steeped is not open.'), liveJoined()];
    void call.useRelay();
    for (let i = 0; i < 100 && !call.problem; i++) await new Promise((r) => setTimeout(r, 20));
    expect(call.problem).toMatch(/not open/);
    followed[0].onState(me({ move: MOVE }));
    await settle(call);
    expect(call.link).toBe('connected');
    expect(call.studio).toBe('live');
    // The old loop stood down: nothing asked for a token on the rehearsal pass after the move.
    expect(events.slice(events.indexOf('fragment rsp-live')).filter((e) => e === 'token rsp-rehearsal')).toEqual([]);
  });
});
