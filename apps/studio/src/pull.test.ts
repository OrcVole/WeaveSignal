// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// A pulled source (1.2, PLAN 17b): the address as it is typed, the station's refusals, its state.
import { describe, expect, it } from 'vitest';
import type { SourceNow } from './lib/api.ts';
import { WHIP_SOUND_ONLY, tileLink, tileRole } from './lib/hosts.ts';
import { pullKindOf, pullProblem, pullRefusal, pullStatus } from './lib/pull.ts';

describe('a pull’s address', () => {
  it('is read as the station reads it: web audio, an HLS playlist, or SRT', () => {
    expect(pullKindOf('https://radio.example.org/live.mp3')).toBe('http');
    expect(pullKindOf(' HTTP://192.0.2.10:8000/Live ')).toBe('http');
    expect(pullKindOf('https://radio.example.org/hls/playlist.M3U8?token=x')).toBe('hls');
    expect(pullKindOf('srt://studio.example.org:9000?streamid=kettle')).toBe('srt');
    expect(pullKindOf('radio.example.org/live')).toBeNull();
    expect(pullKindOf('')).toBeNull();
  });

  it('says what is wrong before the station is asked', () => {
    expect(pullProblem('')).toMatch(/^Give the stream’s address/);
    expect(pullProblem('radio.example.org/live')).toBe('Start the address with http://, https:// or srt://.');
    expect(pullProblem('ftp://radio.example.org/live')).toBe('Start the address with http://, https:// or srt://.');
    expect(pullProblem('https://radio.example.org/live stream')).toBe('An address has no spaces.');
    expect(pullProblem('https://')).toBe('The address has no server name.');
    expect(pullProblem('https://finlay:secret@radio.example.org/live')).toMatch(/^Leave out the user name and password/);
    expect(pullProblem('srt://studio.example.org')).toMatch(/needs a port/);
    expect(pullProblem('srt://studio.example.org:9000?passphrase=abcdefghij')).toMatch(/cannot keep an SRT passphrase yet/);
    expect(pullProblem(`https://radio.example.org/${'a'.repeat(2048)}`)).toBe('An address is at most 2,048 characters.');
    for (const ok of ['https://radio.example.org/live.mp3', 'http://192.0.2.10:8000/live', 'https://radio.example.org/a.m3u8', 'srt://studio.example.org:9000', 'srt://studio.example.org:9000?streamid=kettle']) expect(pullProblem(ok), ok).toBeNull();
  });

  it('puts the station’s own refusals into plainer words, and keeps any other as given', () => {
    expect(pullRefusal("the address points inside the station's own network")).toBe('That address is inside the station’s own network, which the station does not open. Use the stream’s public address.');
    expect(pullRefusal('an SRT passphrase cannot be kept yet: pull from a listener without one, or have the host send to the station with a show key instead')).toMatch(/^The station cannot keep an SRT passphrase yet/);
    expect(pullRefusal('the name `radio.example.org` does not resolve: check the address')).toBe('The name radio.example.org does not resolve: check the address.');
    expect(pullRefusal('the station calls the SRT address, so it must be a listener\'s (leave out `mode`, or set `mode=caller`)')).toMatch(/the other end must be listening/);
    expect(pullRefusal('something new')).toBe('Something new.');
    expect(pullRefusal('')).toBe('The station refused that address.');
  });
});

describe('a pull’s state', () => {
  const now = (o: Partial<SourceNow> = {}): SourceNow => ({ key_id: 'K1', identity: 'source-k1', label: 'Radio Partick', show: 'steeped', show_name: 'Steeped', state: 'standby', slot: 5, way: 'pull', pull: true, format: 'mp3 44.1 kHz stereo', level: -23, leveller_db: 0, steady_ms: 9000, monitor_open: false, door_closes_ms: Date.UTC(2026, 10, 20, 16, 0), title: null, turned_away_until_ms: null, ...o });
  it('says where it stands in one line', () => {
    expect(pullStatus(null, 'UTC')).toMatchObject({ state: 'closed' });
    expect(pullStatus(now(), 'UTC')).toEqual({ state: 'standby', words: 'Open and standing by on the desk · mp3 44.1 kHz stereo' });
    expect(pullStatus(now({ state: 'on_air' }), 'UTC').words).toBe('On air · mp3 44.1 kHz stereo');
    expect(pullStatus(now({ state: 'not_connected', format: null }), 'UTC')).toEqual({ state: 'trying', words: 'Nothing arrives from the address yet. It keeps trying until 16:00.' });
    expect(pullStatus(now({ state: 'test', format: null }), 'UTC').words).toBe('Open as a private test');
  });
});

describe('the tile names the new ways in (1.2)', () => {
  const src = { way: 'pull' as const, format: 'mp3 44.1 kHz stereo', key: 'K1', show: 'steeped', connected_ms: 40_000, leveller_db: 0 };
  it('a pull is a pulled stream, with no claim on how far behind it is', () => {
    expect(tileRole(src, 'Steeped')).toEqual({ short: 'Pulled stream', long: 'Pulled by the station · Steeped' });
    expect(tileLink(src)).toBe('Pull · steady 0:40');
  });
  it('VBAN is a studio PC on the station’s network; the bridge and WHIP are a host’s own studio', () => {
    expect(tileRole({ ...src, way: 'vban' }, 'Steeped')).toEqual({ short: 'Studio PC', long: 'VBAN, on the station’s network · Steeped' });
    expect(tileLink({ ...src, way: 'vban' })).toBe('VBAN · steady 0:40');
    expect(tileRole({ ...src, way: 'bridge' }, 'Steeped')).toEqual({ short: 'Own studio', long: 'The studio app’s bridge · Steeped' });
    expect(tileLink({ ...src, way: 'bridge' })).toBe('Bridge · steady 0:40 · about 2 s behind');
    expect(tileRole({ ...src, way: 'whip' }, 'Steeped')).toEqual({ short: 'Own studio', long: 'WHIP (sound only) · Steeped' });
    expect(tileLink({ ...src, way: 'whip' })).toBe('WHIP · steady 0:40');
  });
  it('WHIP’s one line says the picture is not used', () => {
    expect(WHIP_SOUND_ONLY).toMatch(/^Sound only\./);
    expect(WHIP_SOUND_ONLY).toMatch(/drops it/);
  });
});
