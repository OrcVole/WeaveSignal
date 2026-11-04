// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { AV1_TYPE, OPUS_TYPE, choose, detect, positionAt } from './lib/watch.ts';

const video = { live: '/hls/live.m3u8', audio: '/hls/audio.m3u8' };
const all = { mse: true, av1: true, opusMp4: true };

describe('what the player plays', () => {
  it('plays the picture where the browser decodes AV1 and Opus in fMP4', () => {
    expect(choose(video, '/stream', all, false)).toEqual({ mode: 'video', url: '/hls/live.m3u8', note: null, canToggle: true });
  });

  it('plays the same HLS sound alone when the listener asks (ADR-0026)', () => {
    expect(choose(video, '/stream', all, true)).toMatchObject({ mode: 'audio', url: '/hls/audio.m3u8', canToggle: true });
  });

  it('gives a device without AV1 the sound only, and says why (ADR-0024)', () => {
    const c = choose(video, '/stream', { ...all, av1: false }, false);
    expect(c).toMatchObject({ mode: 'audio', url: '/hls/audio.m3u8', canToggle: false });
    expect(c.note).toMatch(/AV1/);
  });

  it('falls back to the Icecast stream without script playback or Opus in fMP4', () => {
    expect(choose(video, '/stream', { ...all, mse: false }, false)).toMatchObject({ mode: 'icecast', url: '/stream' });
    expect(choose(video, '/stream', { ...all, opusMp4: false }, false)).toMatchObject({ mode: 'icecast', url: '/stream' });
  });

  it('plays the Icecast stream when the station has no video', () => {
    expect(choose(null, '/stream', all, false)).toEqual({ mode: 'icecast', url: '/stream', note: null, canToggle: false });
  });
});

describe('the browser', () => {
  it('asks Media Source for AV1 and Opus, preferring Safari’s ManagedMediaSource', () => {
    const asked: string[] = [];
    const ms = { isTypeSupported: (t: string) => (asked.push(t), t === OPUS_TYPE) };
    expect(detect({ ManagedMediaSource: ms, MediaSource: { isTypeSupported: () => true } })).toEqual({ mse: true, av1: false, opusMp4: true });
    expect(asked).toEqual([AV1_TYPE, OPUS_TYPE]);
    expect(detect({})).toEqual({ mse: false, av1: false, opusMp4: false });
  });
});

describe('switching without a jump', () => {
  const t0 = Date.UTC(2026, 9, 26, 6, 0, 0);
  const frags = [0, 1, 2].map((i) => ({ start: 100 + 2 * i, duration: 2, programDateTime: t0 + 2000 * i }));

  it('finds the same instant by program date-time', () => {
    expect(positionAt(frags, t0 + 3500)).toBeCloseTo(103.5);
    expect(positionAt(frags, t0)).toBeCloseTo(100);
  });

  it('says so when the playlist no longer holds it', () => {
    expect(positionAt(frags, t0 - 1)).toBeNull();
    expect(positionAt(frags, t0 + 6000)).toBeNull();
  });
});
