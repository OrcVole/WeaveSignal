// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { hint, label, pick, type StreamChoice } from './lib/streams.ts';

const opus: StreamChoice = { codec: 'opus', kbps: 96, url: 'https://radio.example/programme.opus' };
const mp3: StreamChoice = { codec: 'mp3', kbps: 128, url: 'https://radio.example/programme.mp3' };
const high: StreamChoice = { codec: 'mp3', kbps: 320, url: 'https://radio.example/programme-high.mp3' };
const all = [opus, mp3, high];

describe('the streams a listener chooses from (ADR-0232)', () => {
  it('names each by codec and rate', () => {
    expect(all.map(label)).toEqual(['Opus, 96 kbit/s', 'MP3, 128 kbit/s', 'MP3, 320 kbit/s']);
  });

  it('says when to choose each', () => {
    expect(hint(opus, all)).toMatch(/best sound/);
    expect(hint(mp3, all)).toMatch(/cannot play Opus/);
    expect(hint(high, all)).toMatch(/music/);
    expect(hint(mp3, [opus, mp3])).toMatch(/cannot play Opus/);
  });

  it('plays Opus unless the listener chose another the station still offers', () => {
    expect(pick(all, null, '')).toBe(opus.url);
    expect(pick(all, high.url, '')).toBe(high.url);
    expect(pick([opus, mp3], high.url, '')).toBe(opus.url);
  });

  it('starts on MP3 where the browser cannot play Ogg Opus', () => {
    expect(pick(all, null, '', false)).toBe(mp3.url);
    expect(pick([opus], null, '', false)).toBe(opus.url);
  });

  it("keeps the station's one address when it lists no streams", () => {
    expect(pick([], null, '/stream')).toBe('/stream');
  });
});
