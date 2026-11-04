// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A guest sees their own camera before they join, and nothing is sent until they press join
// (ADR-0171). The preview and the call are exercised with a fake camera and a fake room.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { LocalVideoTrack } from 'livekit-client';
import type { Joined } from './lib/api.ts';

/** A camera track as livekit-client hands one out, with the calls we care about counted. */
function fakeCamera(live = true) {
  const media = { readyState: live ? 'live' : 'ended' } as MediaStreamTrack;
  const t = {
    mediaStreamTrack: media,
    stop: vi.fn(() => ((media as { readyState: string }).readyState = 'ended')),
    mute: vi.fn(async () => {}),
    unmute: vi.fn(async () => {}),
  };
  return t as unknown as LocalVideoTrack & typeof t;
}

// What the call does with the room, recorded in order.
const events: string[] = [];
let denied = false;
const created: ReturnType<typeof fakeCamera>[] = [];

vi.mock('livekit-client', () => {
  class Room {
    remoteParticipants = new Map();
    canPlaybackAudio = true;
    localParticipant = {
      setTrackSubscriptionPermissions: () => {},
      publishTrack: async (t: unknown, o: { source: string }) => void events.push(`publish ${o.source}`),
    };
    on() {
      return this;
    }
    async connect() {
      events.push('connect');
    }
    async disconnect() {}
  }
  return {
    Room,
    RoomEvent: {},
    DisconnectReason: {},
    Track: { Source: { Camera: 'camera', Microphone: 'microphone' }, Kind: {} },
    createLocalAudioTrack: async () => ({ mediaStreamTrack: {} as MediaStreamTrack, stop: () => {} }),
    createLocalVideoTrack: async () => {
      events.push('capture camera');
      if (denied) throw new DOMException('no', 'NotAllowedError');
      const t = fakeCamera();
      created.push(t);
      return t;
    },
  };
});

vi.mock('./lib/api.ts', async (orig) => ({
  ...(await orig<typeof import('./lib/api.ts')>()),
  followMe: () => () => {},
}));

const joined = (see: string | null = 'desk-pgm'): Joined => ({
  pass: 'p', token: 't', url: 'ws://x', room: 'r', identity: 'ina', role: 'guest', name: 'Ina', hear: 'desk-ina', see, pass_expires_at_ms: Date.now() + 3600_000,
});

beforeEach(() => {
  events.length = 0;
  created.length = 0;
  denied = false;
});

describe('the camera preview before joining', () => {
  it('shows the picture and publishes nothing', async () => {
    const { CameraPreview } = await import('./lib/camera-preview.svelte.ts');
    const cam = fakeCamera();
    const p = new CameraPreview(async () => cam);
    await p.start();
    expect(p.shown).toBe(cam.mediaStreamTrack);
    expect(p.problem).toBeNull();
    expect(p.sendOnJoin).toBe(true); // on by default once joined
    expect(events).toEqual([]); // no room, no publish: it is local
  });

  it('lets go of the camera if they leave without joining', async () => {
    const { CameraPreview } = await import('./lib/camera-preview.svelte.ts');
    const cam = fakeCamera();
    const p = new CameraPreview(async () => cam);
    await p.start();
    p.stop();
    expect(cam.stop).toHaveBeenCalled();
    expect(p.shown).toBeNull();
  });

  it('says so, and can try again, when the camera is refused', async () => {
    const { CameraPreview } = await import('./lib/camera-preview.svelte.ts');
    let n = 0;
    const cam = fakeCamera();
    const p = new CameraPreview(async () => {
      if (n++ === 0) throw new DOMException('no', 'NotAllowedError');
      return cam;
    });
    await p.start();
    expect(p.shown).toBeNull();
    expect(p.problem).toMatch(/did not allow the camera/);
    await p.start();
    expect(p.problem).toBeNull();
    expect(p.shown).toBe(cam.mediaStreamTrack);
  });

  it('hands the same live track over once, with their choice', async () => {
    const { CameraPreview } = await import('./lib/camera-preview.svelte.ts');
    const cam = fakeCamera();
    const p = new CameraPreview(async () => cam);
    await p.start();
    p.sendOnJoin = false;
    const h = p.handOver();
    expect(h).toEqual({ track: cam, on: false, problem: null });
    expect(p.shown).toBeNull();
    p.stop(); // the page closing after the handover must not stop the call's camera
    expect(cam.stop).not.toHaveBeenCalled();
  });

  it('does not hand over a track that has ended', async () => {
    const { CameraPreview } = await import('./lib/camera-preview.svelte.ts');
    const cam = fakeCamera();
    const p = new CameraPreview(async () => cam);
    await p.start();
    (cam.mediaStreamTrack as { readyState: string }).readyState = 'ended';
    expect(p.handOver().track).toBeNull();
  });
});

describe('joining after the preview', () => {
  it('publishes the camera the guest was shown, after connecting and not before, on by default', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    const cam = fakeCamera();
    const call = new Call(joined(), false, { track: cam, on: true, problem: null });
    expect(events).toEqual([]);
    await call.start();
    expect(events).toEqual(['connect', 'publish microphone', 'publish camera']); // no second capture
    expect(call.cameraOn).toBe(true);
    expect(cam.mute).not.toHaveBeenCalled();
    expect(call.camera).toBe(cam.mediaStreamTrack);
  });

  it('joins with the camera muted when the guest chose that', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    const cam = fakeCamera();
    const call = new Call(joined(), false, { track: cam, on: false, problem: null });
    await call.start();
    expect(call.cameraOn).toBe(false);
    expect(cam.mute).toHaveBeenCalled();
  });

  it('joins with sound, and does not ask again, when the camera was refused in the preview', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    denied = true;
    const call = new Call(joined(), false, { track: null, on: true, problem: 'Your camera didn’t start. The studio hears you without your picture.' });
    await call.start();
    expect(events).toEqual(['connect', 'publish microphone']);
    expect(call.link).toBe('connected');
    expect(call.cameraProblem).toMatch(/without your picture/);
  });

  it('stops a previewed camera when video went off before the join', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    const cam = fakeCamera();
    const call = new Call(joined(null), false, { track: cam, on: true, problem: null });
    await call.start();
    expect(cam.stop).toHaveBeenCalled();
    expect(events).toEqual(['connect', 'publish microphone']);
  });

  it('still takes the camera at join when there was no preview', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    const call = new Call(joined());
    await call.start();
    expect(events).toEqual(['capture camera', 'connect', 'publish microphone', 'publish camera']);
    expect(call.cameraOn).toBe(true);
  });

  it('a guest who denies the camera with no preview still joins with sound', async () => {
    const { Call } = await import('./lib/call.svelte.ts');
    denied = true;
    const call = new Call(joined());
    await call.start();
    expect(call.link).toBe('connected');
    expect(call.camera).toBeNull();
    expect(call.cameraProblem).toMatch(/hears you without your picture/);
  });
});
