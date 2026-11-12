// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > RTMP out (ADR-0230; rtmp-out) rendered on the server from a list: each destination
// with its server, formats and the desk's view of it, the warning without video, the add form,
// and never a stream key; and the words and checks behind it.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import Rtmp from './settings/Rtmp.svelte';
import type { RtmpDestinationInfo, RtmpList } from './lib/api.ts';
import { bytesWords, forWords, keyProblem, nameProblem, profileName, stateOf, urlProblem } from './lib/rtmp.ts';

const base = (over: Partial<RtmpDestinationInfo>): RtmpDestinationInfo => ({
  id: '00000000-0000-0000-0000-000000000001',
  name: 'Rumble',
  platform: 'rumble',
  url: 'rtmp://rtmp.rumble.com/live',
  profile: 'compatible',
  enabled: true,
  created_by: 'Morag Campbell',
  created_ms: 0,
  updated_ms: 0,
  key_readable: true,
  desk: null,
  ...over,
});

const desk = (state: string, extra: Record<string, unknown> = {}) => ({
  name: 'x',
  profile: 'compatible',
  enabled: true,
  state,
  connected: state === 'live',
  connected_ms: 125_000,
  last_error: null,
  last_error_ms_ago: null,
  bytes: 48_300_000,
  bytes_total: 48_300_000,
  connects: 1,
  retry_in_ms: null,
  ...extra,
});

const rumble = base({ desk: desk('live') });
const odysee = base({
  id: '00000000-0000-0000-0000-000000000002',
  name: 'Odysee',
  platform: 'odysee',
  url: 'rtmp://publish.odysee.live/live',
  desk: desk('retrying', { last_error: "Failed to connect: 'publish' cmd failed: connection closed remotely", retry_in_ms: 8_000 }),
});
const tube = base({ id: '00000000-0000-0000-0000-000000000003', name: 'PeerTube', platform: 'peertube', url: 'rtmp://tube.example:1935/live', profile: 'open', enabled: false });
const platforms = [
  { platform: 'rumble', name: 'Rumble', url: 'rtmp://rtmp.rumble.com/live', profile: 'compatible', where_to_find: 'Rumble: Go Live, then Get the Streamer Configuration.' },
  { platform: 'custom', name: 'Another RTMP server', url: 'rtmp://server.example/live', profile: 'compatible', where_to_find: 'Your server.' },
];
const list = (over: Partial<RtmpList> = {}): RtmpList => ({
  destinations: [rumble, odysee, tube],
  video: true,
  encoders: [{ profile: 'compatible', encoder: 'openh264', width: 1280, height: 720, fps: 30, video_kbps: 2500, ready: true }],
  platforms,
  ...over,
});

describe('the words for a destination', () => {
  it('says how it is doing, from the desk', () => {
    expect(stateOf(rumble, true)).toEqual({ chip: 'Live', tone: 'live', line: 'Sending for 2 min, 48.3 MB so far.' });
    expect(stateOf(odysee, true).chip).toBe('Retrying');
    expect(stateOf(odysee, true).line).toContain('connection closed remotely');
    expect(stateOf(odysee, true).line).toContain('Trying again in 8 s.');
    expect(stateOf(odysee, true).line).toContain('Check the server and the stream key');
    expect(stateOf(tube, true).chip).toBe('Off');
    expect(stateOf(rumble, false).line).toContain('needs it');
    expect(stateOf(base({ key_readable: false }), true).chip).toBe('Key needed');
    expect(stateOf(base({ desk: null }), true).chip).toBe('Starting');
  });

  it('puts sizes and times simply', () => {
    expect(bytesWords(999)).toBe('999 B');
    expect(bytesWords(12_400)).toBe('12 kB');
    expect(bytesWords(2_340_000_000)).toBe('2.34 GB');
    expect(forWords(59_000)).toBe('59 s');
    expect(forWords(3_720_000)).toBe('1 h 2 min');
    expect(profileName('open')).toBe('AV1 and Opus');
  });

  it('checks the form before asking', () => {
    expect(urlProblem('rtmp://rtmp.rumble.com/live')).toBeNull();
    expect(urlProblem('rtmps://tube.example:1936/live')).toBeNull();
    expect(urlProblem('https://rumble.com')).toContain('rtmp://');
    expect(urlProblem('rtmp://a/live?key=1')).toContain('own field');
    expect(keyProblem('', false)).toContain('Paste');
    expect(keyProblem('', true)).toBeNull();
    expect(keyProblem('a b', false)).toContain('no spaces');
    expect(nameProblem('Rumble main')).toBeNull();
    expect(nameProblem('a/b')).toContain('letters');
  });
});

describe('Settings > RTMP out', () => {
  it('lists each destination with its server, formats and state, and never a key', () => {
    const { body } = render(Rtmp, { props: { initial: list() } });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(body).toContain('data-panel="settings.rtmp"');
    expect(PANELS['settings.rtmp']).toBeTruthy();
    expect(text).toContain('1 destination live');
    for (const d of [rumble, odysee, tube]) {
      expect(text).toContain(d.name);
      expect(text).toContain(d.url);
    }
    expect(text).toContain('H.264 and AAC');
    expect(text).toContain('AV1 and Opus');
    expect(text).toContain('Sending for 2 min');
    expect(text).toContain('Trying again in 8 s.');
    expect(body).toContain('Turn off<span class="sr-only"> Rumble</span>');
    expect(body).toContain('Turn on<span class="sr-only"> PeerTube</span>');
    expect(text).toContain('Encoding now:');
    expect(text).toContain('1280×720, 30 fps, 2500 kbit/s (openh264)');
    // The key field is a password field and starts empty.
    expect(body).toMatch(/<input id="rt-key" type="password"[^>]*>/);
    expect(body).not.toMatch(/id="rt-key"[^>]*value="[^"]+"/);
  });

  it('warns without video, and turning on waits for it', () => {
    const { body } = render(Rtmp, { props: { initial: list({ video: false, encoders: [] }) } });
    const text = body.replace(/<!--[^>]*-->/g, '');
    expect(text).toContain('The desk runs without video, and RTMP out needs it');
    expect(body).toMatch(/<button[^>]*disabled[^>]*>Turn on<span class="sr-only"> PeerTube/);
  });

  it('says what to do when there are none', () => {
    const { body } = render(Rtmp, { props: { initial: list({ destinations: [] }) } });
    expect(body.replace(/<!--[^>]*-->/g, '')).toContain('No destinations yet.');
  });
});
