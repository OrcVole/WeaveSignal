// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { ECHO_TEST_CAPTURE } from '../../../packages/ui/media.ts';
import { check, firstError, request } from './lib/lobby.ts';
import { barPct, dbfs, heard } from './lib/meter.ts';
import { cameraProblem, micProblem } from './lib/mic.ts';
import { PLACE_HELD_MS, backoff, cameraCapture, connectOptions, limitSubscribers, offersCamera, rejoinUntil, shouldHear, shouldSee, voiceCapture, wantsCameraPreview } from './lib/rules.ts';
import { kept } from './lib/store.ts';
import { installStationIce, stationIceServers, useStationIce, withoutOutsideStun, withTurnPort } from './lib/turn.ts';

const lim = { name_max: 60, topic_max: 140 };

describe('the lobby: consent is required', () => {
  it('a call with no consent is no request at all', () => {
    const f = { name: 'Jessie Brown', topic: 'No bus after 23:10', consent: false };
    expect(request(f, lim, 'v1')).toBeNull();
    expect(check(f, lim).consent).toMatch(/broadcast and recorded/);
    expect(firstError(check(f, lim))).toBe('consent');
  });
  it('with consent, the request carries it and the words’ version', () => {
    const r = request({ name: '  Jessie  ', topic: ' Buses ', consent: true }, lim, '0123456789abcdef');
    expect(r).toEqual({ name: 'Jessie', topic: 'Buses', consent: { accepted: true, version: '0123456789abcdef' } });
  });
  it('a name and a topic are needed, within bounds; errors come in reading order', () => {
    const e = check({ name: ' ', topic: '', consent: false }, lim);
    expect(Object.keys(e)).toEqual(['name', 'topic', 'consent']);
    expect(firstError(e)).toBe('name');
    expect(check({ name: 'x'.repeat(61), topic: 't', consent: true }, lim).name).toMatch(/60/);
    expect(check({ name: 'Iona', topic: 'y'.repeat(141), consent: true }, lim).topic).toMatch(/140/);
    expect(check({ name: 'Iona', topic: 'Ferries', consent: true }, lim)).toEqual({});
  });
});

describe('the room rules', () => {
  it('every join page cancels echo, suppresses noise and leaves gain alone', () => {
    expect(voiceCapture()).toEqual({ echoCancellation: true, noiseSuppression: true, autoGainControl: false });
    expect(ECHO_TEST_CAPTURE.echoCancellation).toBe(false);
  });
  it('a page hears only its own mix-minus, never another’s', () => {
    expect(shouldHear('desk-mm-caller-1a2b', 'desk-mm-caller-1a2b')).toBe(true);
    expect(shouldHear('desk-mm-caller-9z9z', 'desk-mm-caller-1a2b')).toBe(false);
    expect(shouldHear('rory', 'desk-mm-caller-1a2b')).toBe(false);
    expect(shouldHear('desk-mm-caller-1a2b-x', 'desk-mm-caller-1a2b')).toBe(false);
  });
  it('lets no one in the room subscribe to its own tracks', () => {
    const calls: unknown[][] = [];
    limitSubscribers({ setTrackSubscriptionPermissions: (...args: unknown[]) => void calls.push(args) });
    expect(calls).toEqual([[false, []]]);
  });
  it('never auto-subscribes, and the relay means TURN only', () => {
    expect(connectOptions(false).autoSubscribe).toBe(false);
    expect(connectOptions(false).rtcConfig).toBeUndefined();
    expect(connectOptions(true).rtcConfig).toEqual({ iceTransportPolicy: 'relay' });
  });
  it('a dropped caller rejoins for 5 minutes; anyone else while the pass lasts', () => {
    expect(rejoinUntil('caller', 1000, 10_000_000)).toBe(1000 + PLACE_HELD_MS);
    expect(rejoinUntil('caller', 1000, 2000)).toBe(2000);
    expect(rejoinUntil('guest', 1000, 10_000_000)).toBe(10_000_000);
    expect([0, 1, 2, 3, 4, 9].map(backoff)).toEqual([1000, 2000, 4000, 8000, 10_000, 10_000]);
  });
  it('a kept call is offered back only while its place may be held', () => {
    const k = { pass: 'rsp1.x.y', identity: 'caller-1', name: 'Iona', seen_ms: 1000, pass_expires_at_ms: 1e12 };
    expect(kept(1000 + PLACE_HELD_MS - 1, JSON.stringify(k))?.identity).toBe('caller-1');
    expect(kept(1000 + PLACE_HELD_MS + 1, JSON.stringify(k))).toBeNull();
    expect(kept(5000, '{nonsense')).toBeNull();
    expect(kept(5000, null)).toBeNull();
  });
});

describe('the microphone check', () => {
  it('measures a block and places it on the bar', () => {
    expect(dbfs(new Float32Array(480))).toBe(-Infinity);
    expect(dbfs(new Float32Array(480).fill(0.5))).toBeCloseTo(-6.02, 1);
    expect(barPct(-Infinity)).toBe(0);
    expect(barPct(-30)).toBe(50);
    expect(barPct(3)).toBe(100);
    expect(heard(-40)).toBe(true);
    expect(heard(-70)).toBe(false);
  });
  it('says what to do when the microphone is refused', () => {
    expect(micProblem(new DOMException('no', 'NotAllowedError'))).toMatch(/Allow it/);
    expect(micProblem(new DOMException('no', 'NotFoundError'))).toMatch(/No microphone/);
    expect(micProblem(new Error('x'))).toMatch(/could not be started/);
  });
});

describe("the relay's port (LiveKit advertises TURN over TLS on 443 whatever its port)", () => {
  const lk: RTCIceServer[] = [{ urls: ['turns:turn.example.org:443?transport=tcp', 'turn:turn.example.org:3478?transport=udp'], username: 'u', credential: 'c' }];
  it('moves only the TURN-over-TLS URL to the station port', () => {
    expect(withTurnPort(lk, 5349)).toEqual([{ urls: ['turns:turn.example.org:5349?transport=tcp', 'turn:turn.example.org:3478?transport=udp'], username: 'u', credential: 'c' }]);
    expect(withTurnPort([{ urls: 'turns:t.example.org:443?transport=tcp' }], 15349)).toEqual([{ urls: 'turns:t.example.org:15349?transport=tcp' }]);
  });
  it('leaves 443, and anything else, alone', () => {
    expect(withTurnPort(lk, 443)).toBe(lk);
    expect(withTurnPort([{ urls: 'turns:t.example.org:5349?transport=tcp' }], 15349)).toEqual([{ urls: 'turns:t.example.org:5349?transport=tcp' }]);
    expect(withTurnPort(undefined, 5349)).toBeUndefined();
  });
});

describe('the picture (ADR-0028)', () => {
  it('watches only the desk’s programme picture, and nothing with video off', () => {
    expect(shouldSee('desk-pgm', 'desk-pgm')).toBe(true);
    expect(shouldSee('gregor', 'desk-pgm')).toBe(false);
    expect(shouldSee('desk-pgm', null)).toBe(false);
  });

  it('offers the camera to the people who appear, only while the desk has video on', () => {
    for (const r of ['presenter', 'dj', 'guest']) expect(offersCamera(r, 'desk-pgm')).toBe(true);
    for (const r of ['caller', 'screener', 'producer']) expect(offersCamera(r, 'desk-pgm')).toBe(false);
    expect(offersCamera('guest', null)).toBe(false);
  });

  it('captures at the desk’s picture size and rate', () => {
    expect(cameraCapture().resolution).toEqual({ width: 854, height: 480, frameRate: 20 });
  });

  it('shows a person their own picture before they join, only if they appear and video is on (ADR-0171)', () => {
    for (const r of ['presenter', 'dj', 'guest']) {
      expect(wantsCameraPreview(r, true)).toBe(true);
      expect(wantsCameraPreview(r, false)).toBe(false);
    }
    for (const r of ['caller', 'screener', 'producer']) expect(wantsCameraPreview(r, true)).toBe(false);
  });

  it('says why a camera did not start, and that joining with sound still works', () => {
    const denied = new DOMException('no', 'NotAllowedError');
    expect(cameraProblem(denied)).toMatch(/did not allow the camera/);
    expect(cameraProblem(denied)).toMatch(/You can still join/);
    expect(cameraProblem(new DOMException('', 'NotFoundError'))).toMatch(/No camera/);
    expect(cameraProblem(new DOMException('', 'NotReadableError'))).toMatch(/Another app/);
    expect(cameraProblem(new Error('x'))).toMatch(/didn't start/);
    // Inside the call they have already joined.
    expect(cameraProblem(denied, true)).not.toMatch(/still join/);
    expect(cameraProblem(denied, true)).toMatch(/hears you without your picture/);
  });
});

describe('no outside STUN server unless the station named one (ADR-0290)', () => {
  // What LiveKit 1.13.10 sends with no STUN and no UDP TURN configured: Twilio's and Google's.
  const defaults: RTCIceServer = { urls: ['stun:global.stun.twilio.com:3478', 'stun:stun.l.google.com:19302', 'stun:stun1.l.google.com:19302'] };
  const turn: RTCIceServer = { urls: ['turns:turn.example.org:443?transport=tcp'], username: 'u', credential: 'c' };

  it("drops LiveKit's default STUN servers and keeps TURN", () => {
    expect(withoutOutsideStun([defaults, turn], [])).toEqual([turn]);
    expect(withoutOutsideStun([defaults], [])).toEqual([]);
    expect(withoutOutsideStun(undefined, [])).toEqual([]);
  });
  it('keeps the STUN URLs in a mixed server whose TURN URLs it must not touch', () => {
    const mixed: RTCIceServer = { urls: ['turn:t.example.org:3478?transport=udp', 'stun:stun.l.google.com:19302'], username: 'u', credential: 'c' };
    expect(withoutOutsideStun([mixed], [])).toEqual([{ urls: ['turn:t.example.org:3478?transport=udp'], username: 'u', credential: 'c' }]);
  });
  it('lets through only the servers the station named, whatever the URL looks like', () => {
    const named = ['stun.example.org:3478'];
    const out = withoutOutsideStun([{ urls: 'stun:STUN.example.org:3478' }, { urls: ['stuns:stun.example.org:5349', 'stun://other.example.net:3478?transport=udp'] }, defaults], named);
    expect(out).toEqual([{ urls: 'stun:STUN.example.org:3478' }]);
    // An address with no port is the scheme's default, so a station's server on 3478 matches.
    expect(withoutOutsideStun([{ urls: 'stun:stun.example.org' }], named)).toEqual([{ urls: 'stun:stun.example.org' }]);
    // `stuns:` defaults to 5349, a different server, so it goes and the named one is added.
    expect(withoutOutsideStun([{ urls: 'stuns:stun.example.org' }], named)).toEqual([{ urls: ['stun:stun.example.org:3478'] }]);
  });
  it('judges STUN by the scheme alone: a URL it cannot read is dropped, not kept', () => {
    const odd: RTCIceServer = { urls: ['stun:stun.l.google.com:19302/', 'STUN:Stun.l.google.com', 'stuns:stun.l.google.com:5349 ', 'stun:', 'stun://[2001:db8::1]:3478', 'turn:t.example.org:3478'] };
    expect(withoutOutsideStun([odd], [])).toEqual([{ urls: ['turn:t.example.org:3478'] }]);
  });
  it('adds the servers the station named when LiveKit sent none of them', () => {
    expect(withoutOutsideStun([defaults, turn], ['stun.example.org:3478'])).toEqual([turn, { urls: ['stun:stun.example.org:3478'] }]);
  });
  it('does both with the relay: the port, and no outside STUN', () => {
    const out = stationIceServers([defaults, turn], { turn_tls_port: 5349, stun_servers: [] });
    expect(out).toEqual([{ urls: ['turns:turn.example.org:5349?transport=tcp'], username: 'u', credential: 'c' }]);
    // Without the station's facts the page still asks no STUN server, and TURN stays as sent.
    expect(stationIceServers([defaults, turn], undefined)).toEqual([turn]);
    expect(stationIceServers(undefined, undefined)).toBeUndefined();
  });
  it('is on every peer connection the page opens, from the first call whatever it is told', () => {
    const seen: (RTCConfiguration | undefined)[] = [];
    class Fake {
      constructor(config?: RTCConfiguration) {
        seen.push(config);
      }
      setConfiguration(config?: RTCConfiguration): void {
        seen.push(config);
      }
    }
    const before = globalThis.RTCPeerConnection;
    try {
      globalThis.RTCPeerConnection = Fake as unknown as typeof RTCPeerConnection;
      useStationIce(undefined);
      const pc = new RTCPeerConnection({ iceServers: [defaults, turn] });
      pc.setConfiguration({ iceServers: [defaults, turn] });
      expect(seen).toEqual([{ iceServers: [turn] }, { iceServers: [turn] }]);
      // Facts that arrive later are read by the connections made after them.
      useStationIce({ turn_tls_port: 5349, stun_servers: ['stun.example.org:3478'] });
      seen.length = 0;
      new RTCPeerConnection({ iceServers: [defaults, turn], iceTransportPolicy: 'relay' });
      expect(seen).toEqual([
        { iceServers: [{ urls: ['turns:turn.example.org:5349?transport=tcp'], username: 'u', credential: 'c' }, { urls: ['stun:stun.example.org:3478'] }], iceTransportPolicy: 'relay' },
      ]);
      // Installing again, as a room does beside `new Room`, keeps the facts the page was given.
      installStationIce();
      seen.length = 0;
      new RTCPeerConnection({ iceServers: [defaults, turn] });
      expect(seen).toEqual([{ iceServers: [{ urls: ['turns:turn.example.org:5349?transport=tcp'], username: 'u', credential: 'c' }, { urls: ['stun:stun.example.org:3478'] }] }]);
    } finally {
      globalThis.RTCPeerConnection = before;
    }
  });
});
