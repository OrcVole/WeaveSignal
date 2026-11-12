// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { describe, expect, it } from 'vitest';
import { lookOf } from './lib/brand.svelte.ts';

const icon = { url: '/api/v1/branding/icon?v=0123456789abcdef', width: 256, height: 256, bytes: 4000 };
const banner = { url: '/api/v1/branding/banner?v=fedcba9876543210', width: 1200, height: 400, bytes: 90000 };

describe("the station's look as the pages take it", () => {
  it('takes what the station sends', () => {
    expect(lookOf({ accent: '#B4532A', icon, banner })).toEqual({
      accent: '#b4532a',
      icon: { url: icon.url, width: 256, height: 256 },
      banner: { url: banner.url, width: 1200, height: 400 },
    });
  });
  it('is nothing for a station that has set nothing, or for anything that is not a look', () => {
    const none = { accent: null, icon: null, banner: null };
    for (const v of [{ accent: null, icon: null, banner: null }, null, undefined, 'x', 7, [], {}, { error: 'no such route' }]) {
      expect(lookOf(v)).toEqual(none);
    }
  });
  it("shows a picture only from this station's own branding routes", () => {
    for (const url of [
      'https://elsewhere.example/icon.png',
      '//elsewhere.example/api/v1/branding/icon?v=0123456789abcdef',
      'javascript:alert(1)',
      'data:image/svg+xml,<svg onload=alert(1)>',
      '/api/v1/branding/icon?v=0123456789abcdef&x=1',
      '/api/v1/branding/icon',
      '/api/v1/branding/banner?v=0123456789abcdef',
      '/api/v1/setup',
    ]) {
      expect(lookOf({ icon: { ...icon, url } }).icon, url).toBeNull();
    }
    expect(lookOf({ icon: { ...icon, width: 0 } }).icon).toBeNull();
    expect(lookOf({ icon: { ...icon, height: 'tall' } }).icon).toBeNull();
  });
  it('takes a colour only as six or three hex digits', () => {
    expect(lookOf({ accent: 'red; background: url(//elsewhere.example)' }).accent).toBeNull();
    expect(lookOf({ accent: 7 }).accent).toBeNull();
    expect(lookOf({ accent: '#fa0' }).accent).toBe('#ffaa00');
  });
});
