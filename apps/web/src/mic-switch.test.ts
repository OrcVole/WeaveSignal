// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The microphone during a call (1.3.3), with a fake room and a fake browser: what the browser is
// asked for at the start, a change that keeps the one publication (nothing unpublished, nothing
// rejoined), a change that cannot be had leaving the microphone in use running, a microphone
// that is unplugged, one that comes back, and a rejoin after a change.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Joined } from './lib/api.ts';
import { type Mic, MIC_STOPPED, forgetUsed } from './lib/mic-devices.ts';

const BUILT_IN: Mic = { id: 'built-in', label: 'Built-in Audio' };
const HEADSET: Mic = { id: 'headset', label: 'USB headset' };
const STUDIO_MIC: Mic = { id: 'studio-mic', label: 'Studio microphone' };

/** What the call does with the room and the browser, in order. */
const events: string[] = [];
/** The microphones plugged in, the system's own first. */
let plugged: Mic[] = [];
/** A microphone the browser will not give, and why. */
let refuse: Record<string, string> = {};
/** Every capture asked of the browser, as asked. */
let asked: MediaTrackConstraints[] = [];
let deviceChange: (() => void) | null = null;
const store = new Map<string, string>();
let tokens: Joined[] = [];

/** A capture: a MediaStreamTrack as far as the call reads one. */
class Capture {
  readyState = 'live';
  label: string;
  #id: string;
  #constraints: MediaTrackConstraints;
  #ended: (() => void)[] = [];
  constructor(mic: Mic, constraints: MediaTrackConstraints) {
    this.label = mic.label;
    this.#id = mic.id;
    this.#constraints = constraints;
  }
  getSettings() {
    return { deviceId: this.#id, groupId: `g-${this.#id}` };
  }
  getConstraints() {
    return this.#constraints;
  }
  addEventListener(type: string, fn: () => void) {
    if (type === 'ended') this.#ended.push(fn);
  }
  stop() {
    this.readyState = 'ended';
  }
  /** Unplugged: the browser ends the track and says so. */
  end() {
    this.readyState = 'ended';
    for (const fn of this.#ended) fn();
  }
}

/** The browser's answer to a capture, as Chromium gives it: an exact id or nothing, and its own
 *  first microphone for anything else (a preferred id is ignored). */
function capture(audio: MediaTrackConstraints): Capture {
  asked.push(audio);
  const want = audio.deviceId as { exact?: string } | undefined;
  const exact = want?.exact ? plugged.find((m) => m.id === want.exact) : undefined;
  if (want?.exact && !exact) throw new DOMException('gone', 'OverconstrainedError');
  const mic = exact ?? plugged[0];
  if (!mic) throw new DOMException('none', 'NotFoundError');
  if (refuse[mic.id]) throw new DOMException('no', refuse[mic.id]);
  events.push(`capture ${mic.label}`);
  return new Capture(mic, audio);
}

/** livekit-client's LocalAudioTrack as far as the call uses one. */
class FakeTrack {
  mediaStreamTrack: Capture;
  sender = {};
  isMuted = false;
  stopped = false;
  #on = new Map<string, Set<() => void>>();
  constructor(c: Capture) {
    this.mediaStreamTrack = c;
  }
  on(e: string, fn: () => void) {
    if (!this.#on.has(e)) this.#on.set(e, new Set());
    this.#on.get(e)!.add(fn);
    return this;
  }
  off(e: string, fn: () => void) {
    this.#on.get(e)?.delete(fn);
    return this;
  }
  emit(e: string) {
    for (const fn of this.#on.get(e) ?? []) fn();
  }
  async replaceTrack(next: Capture, o: { userProvidedTrack: boolean }) {
    events.push(`replace with ${next.label} userProvided=${o.userProvidedTrack}`);
    if (failReplace) throw new Error('unable to replace an unpublished track');
    // As livekit-client does for a track it manages: the sender takes the new one, then the old one stops.
    const old = this.mediaStreamTrack;
    this.mediaStreamTrack = next;
    old.stop();
    return this;
  }
  async unmute() {
    events.push('unmute');
    this.isMuted = false;
    return this;
  }
  stop() {
    this.stopped = true;
    this.mediaStreamTrack.stop();
  }
}
let failReplace = false;
let made: FakeTrack[] = [];
let rooms: { localParticipant: { activeDeviceMap: Map<string, string> } }[] = [];

vi.mock('livekit-client', () => {
  class Room {
    remoteParticipants = new Map();
    canPlaybackAudio = true;
    localParticipant = {
      activeDeviceMap: new Map<string, string>([['audioinput', 'default']]),
      setTrackSubscriptionPermissions: () => {},
      publishTrack: async (t: FakeTrack) => void events.push(`publish ${t === made[0] ? 'the first track' : 'another track'} on ${t.mediaStreamTrack.label}`),
    };
    constructor() {
      rooms.push(this);
    }
    on() {
      return this;
    }
    async connect() {
      events.push('connect');
    }
    async disconnect(stopTracks?: boolean) {
      events.push(`disconnect stopTracks=${stopTracks}`);
    }
  }
  return {
    Room,
    RoomEvent: {},
    TrackEvent: { Restarted: 'restarted', Muted: 'muted' },
    DisconnectReason: {},
    Track: { Source: { Camera: 'camera', Microphone: 'microphone' }, Kind: {} },
    // As livekit-client does: a bare id is exact, and if that microphone is not there it asks
    // again with the id as a preference.
    createLocalAudioTrack: async (o: MediaTrackConstraints & { deviceId?: string }) => {
      let c: Capture;
      try {
        c = capture(typeof o.deviceId === 'string' ? { ...o, deviceId: { exact: o.deviceId } } : o);
      } catch (e) {
        if (typeof o.deviceId !== 'string' || (e as DOMException).name !== 'OverconstrainedError') throw e;
        c = capture({ ...o, deviceId: { ideal: o.deviceId } });
      }
      const t = new FakeTrack(c);
      made.push(t);
      return t;
    },
    createLocalVideoTrack: async () => {
      throw new Error('no camera in these tests');
    },
  };
});

vi.mock('./lib/api.ts', async (orig) => ({
  ...(await orig<typeof import('./lib/api.ts')>()),
  followMe: () => () => {},
  passToken: async () => {
    events.push('token');
    const j = tokens.shift();
    if (!j) throw new Error('no token');
    return j;
  },
}));

const joined = (): Joined => ({ pass: 'p', token: 't', url: 'ws://x', room: 'studio', identity: 'gregor', role: 'guest', name: 'Gregor Stewart', hear: 'desk-mm-gregor', see: null, pass_expires_at_ms: Date.now() + 3600_000 });
const RULES = { echoCancellation: true, noiseSuppression: true, autoGainControl: false };

beforeEach(() => {
  events.length = 0;
  plugged = [BUILT_IN, HEADSET, STUDIO_MIC];
  refuse = {};
  asked = [];
  deviceChange = null;
  failReplace = false;
  made = [];
  rooms = [];
  tokens = [];
  store.clear();
  forgetUsed();
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
  vi.stubGlobal('navigator', {
    mediaDevices: {
      // Named, as after the page has been given a microphone.
      enumerateDevices: async () => plugged.map((m) => ({ kind: 'audioinput', deviceId: m.id, label: m.label, groupId: `g-${m.id}` })),
      getUserMedia: async (c: { audio: MediaTrackConstraints }) => {
        const t = capture(c.audio);
        return { getAudioTracks: () => [t] };
      },
      addEventListener: (_: string, fn: () => void) => void (deviceChange = fn),
      removeEventListener: () => void (deviceChange = null),
    },
  });
});

async function inCall() {
  const { Call } = await import('./lib/call.svelte.ts');
  const call = new Call(joined());
  await call.start();
  expect(call.link).toBe('connected');
  return call;
}
const tick = () => new Promise((r) => setTimeout(r, 0));
const remembered = (): Mic | null => JSON.parse(store.get('weavesignal.mic') ?? 'null');

describe('the microphone a call starts with', () => {
  it('with no choice kept, takes the browser’s own with the join pages’ rules, and names it', async () => {
    const call = await inCall();
    expect(asked).toEqual([RULES]);
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(call.mics.list).toEqual([BUILT_IN, HEADSET, STUDIO_MIC]);
    expect(call.micNote).toBeNull();
    expect(remembered()).toBeNull();
  });

  it('asks for the microphone chosen on this device exactly, with the same rules', async () => {
    store.set('weavesignal.mic', JSON.stringify(STUDIO_MIC));
    const call = await inCall();
    expect(asked).toEqual([{ ...RULES, deviceId: { exact: 'studio-mic' } }]);
    expect(call.micInUse).toEqual(STUDIO_MIC);
    expect(call.micNote).toBeNull();
  });

  it('falls back to the browser’s own when the chosen microphone is gone, says so, and keeps the choice for when it is back', async () => {
    store.set('weavesignal.mic', JSON.stringify(STUDIO_MIC));
    plugged = [BUILT_IN, HEADSET];
    const call = await inCall();
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(call.micNote).toBe('The microphone you chose before, Studio microphone, is not connected. This page is using Built-in Audio instead.');
    expect(call.micTrouble).toBe(false);
    expect(remembered()).toEqual(STUDIO_MIC);
    expect(events).toEqual(['capture Built-in Audio', 'connect', 'publish the first track on Built-in Audio']);
  });
});

describe('changing microphone during a call', () => {
  it('captures the new one first and puts it on the same published track: nothing is unpublished, rejoined or asked of the station', async () => {
    const call = await inCall();
    const before = call.mic;
    events.length = 0;
    await call.useMic('headset');
    expect(events).toEqual(['capture USB headset', 'replace with USB headset userProvided=false']);
    // Exactly that microphone, with the rules, and whatever the track in use was captured with.
    expect(asked[1]).toEqual({ ...RULES, deviceId: { exact: 'headset' } });
    expect(call.micInUse).toEqual(HEADSET);
    expect(call.mic).not.toBe(before);
    expect((call.mic as unknown as Capture).label).toBe(HEADSET.label);
    expect((before as unknown as Capture).readyState).toBe('ended');
    expect(made).toHaveLength(1);
    expect(made[0].stopped).toBe(false);
    expect(call.micNote).toBe('Now using USB headset. The studio hears you through it.');
    expect(call.micTrouble).toBe(false);
    expect(call.micChanging).toBe(false);
    expect(remembered()).toEqual(HEADSET);
    // The room client is told which microphone this is, so it does not take it for the system's own.
    expect(rooms[0].localParticipant.activeDeviceMap.get('audioinput')).toBe('headset');
  });

  it('a microphone the browser refuses leaves the one in use running, says so, and remembers nothing', async () => {
    const call = await inCall();
    const before = call.mic as unknown as Capture;
    refuse = { headset: 'NotAllowedError' };
    events.length = 0;
    await call.useMic('headset');
    expect(events).toEqual([]);
    expect(before.readyState).toBe('live');
    expect(call.mic).toBe(before);
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(call.micNote).toBe('The browser did not allow USB headset, so this page is still using Built-in Audio. Allow it when the browser asks, then choose it again.');
    expect(call.micTrouble).toBe(true);
    expect(remembered()).toBeNull();
  });

  it('a microphone unplugged between the list and the press is said, and the one in use carries on', async () => {
    const call = await inCall();
    plugged = [BUILT_IN, STUDIO_MIC];
    await call.useMic('headset');
    expect(call.micNote).toBe('USB headset is no longer connected, so this page is still using Built-in Audio.');
    expect((call.mic as unknown as Capture).readyState).toBe('live');
  });

  it('a capture that cannot be put on the track is let go, and the one in use carries on', async () => {
    const call = await inCall();
    failReplace = true;
    await call.useMic('studio-mic');
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(call.micTrouble).toBe(true);
    expect(call.micNote).toBe('Studio microphone could not be started, so this page is still using Built-in Audio.');
    // The capture taken for the change does not stay open with nothing using it.
    expect(asked).toHaveLength(2);
    expect((call.mic as unknown as Capture).label).toBe(BUILT_IN.label);
  });

  it('does nothing while the page is not connected, for the microphone already in use, or for one not in the list', async () => {
    const call = await inCall();
    events.length = 0;
    await call.useMic('built-in');
    await call.useMic('never-listed');
    call.link = 'reconnecting';
    await call.useMic('headset');
    expect(events).toEqual([]);
    expect(asked).toHaveLength(1);
  });

  it('a rejoin after a change publishes the same track again, on the microphone chosen: no fresh capture', async () => {
    const call = await inCall();
    await call.useMic('studio-mic');
    events.length = 0;
    tokens = [joined()];
    await call.useRelay();
    expect(events).toEqual(['disconnect stopTracks=false', 'token', 'connect', 'publish the first track on Studio microphone']);
    expect(made).toHaveLength(1);
  });
});

describe('microphones plugged in and taken away during a call', () => {
  it('keeps the list current', async () => {
    const call = await inCall();
    plugged = [BUILT_IN];
    deviceChange?.();
    await tick();
    expect(call.mics.list).toEqual([BUILT_IN]);
    plugged = [BUILT_IN, HEADSET];
    deviceChange?.();
    await tick();
    expect(call.mics.list).toEqual([BUILT_IN, HEADSET]);
    // Nothing moved: a microphone that appears is offered, never switched to while one is sending.
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(asked).toHaveLength(1);
  });

  it('a chosen microphone that comes back while another is sending is offered, not switched to', async () => {
    store.set('weavesignal.mic', JSON.stringify(STUDIO_MIC));
    plugged = [BUILT_IN];
    const call = await inCall();
    plugged = [BUILT_IN, STUDIO_MIC];
    deviceChange?.();
    await tick();
    expect(call.mics.list).toEqual([BUILT_IN, STUDIO_MIC]);
    expect(call.micInUse).toEqual(BUILT_IN);
    expect(asked).toHaveLength(1);
  });

  it('when the one in use is unplugged and the room client starts the track on another, the page follows and says which', async () => {
    store.set('weavesignal.mic', JSON.stringify(HEADSET));
    const call = await inCall();
    expect(call.micInUse).toEqual(HEADSET);
    // Unplugged: the browser ends the capture; livekit-client restarts the track on the system's own.
    plugged = [BUILT_IN, STUDIO_MIC];
    (call.mic as unknown as Capture).end();
    made[0].mediaStreamTrack = new Capture(BUILT_IN, { ...RULES, deviceId: { exact: 'default' } });
    made[0].emit('restarted');
    await tick();
    expect(call.micInUse).toEqual(BUILT_IN);
    expect((call.mic as unknown as Capture).label).toBe(BUILT_IN.label);
    expect(call.micNote).toBe('USB headset stopped, so this page is now using Built-in Audio.');
    expect(call.micStopped).toBe(false);
    // The choice is kept for the next time the headset is there.
    expect(remembered()).toEqual(HEADSET);
  });

  it('when nothing could take its place the page says the studio cannot hear, and takes the first microphone that appears', async () => {
    plugged = [HEADSET];
    const call = await inCall();
    plugged = [];
    (call.mic as unknown as Capture).end();
    made[0].isMuted = true;
    made[0].emit('muted');
    await tick();
    expect(call.micStopped).toBe(true);
    expect(call.micNote).toBe(MIC_STOPPED);
    expect(call.micTrouble).toBe(true);
    events.length = 0;
    plugged = [STUDIO_MIC];
    deviceChange?.();
    for (let i = 0; i < 20 && call.micStopped; i++) await tick();
    expect(events).toEqual(['capture Studio microphone', 'replace with Studio microphone userProvided=false', 'unmute']);
    expect(call.micStopped).toBe(false);
    expect(call.micInUse).toEqual(STUDIO_MIC);
    expect(call.micNote).toBe('This page is now using Studio microphone.');
    // Taken because nothing else was sending, not chosen: nothing is remembered.
    expect(remembered()).toBeNull();
  });

  it('with none sending, the person can choose one from the list themselves', async () => {
    const call = await inCall();
    (call.mic as unknown as Capture).end();
    made[0].isMuted = true;
    made[0].emit('muted');
    await tick();
    events.length = 0;
    await call.useMic('built-in');
    expect(events).toEqual(['capture Built-in Audio', 'replace with Built-in Audio userProvided=false', 'unmute']);
    expect(call.micStopped).toBe(false);
    expect(call.micNote).toBe('Now using Built-in Audio. The studio hears you through it.');
  });
});
