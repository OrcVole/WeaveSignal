// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The theme toggle (ADR-0065, ADR-0270): the frame offers Dark, Light and Night; choosing Night
// puts it on the document and remembers it; and no Night token carries a blue or cyan hue.
import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import { THEMES, applyTheme, setTheme, theme, toggleTheme } from './lib/theme.svelte.ts';

function fakeDocument() {
  const dataset: Record<string, string> = {};
  const kept = new Map<string, string>();
  vi.stubGlobal('document', { documentElement: { dataset } });
  vi.stubGlobal('localStorage', { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => void kept.set(k, v) });
  return { dataset, kept };
}

afterEach(() => {
  vi.unstubAllGlobals();
  theme.current = 'dark';
});

describe('the theme toggle', () => {
  it('offers Dark, Light and Night in the frame, Dark pressed at first', () => {
    const { body } = render(App);
    const group = body.slice(body.indexOf('aria-label="Theme"'));
    const toggle = group.slice(0, group.indexOf('</div>'));
    expect([...toggle.matchAll(/aria-pressed="(true|false)"[^>]*>([A-Za-z]+)</g)].map((m) => [m[2], m[1]])).toEqual([
      ['Dark', 'true'],
      ['Light', 'false'],
      ['Night', 'false'],
    ]);
  });

  it('sets Night on the document and remembers it', () => {
    const { dataset, kept } = fakeDocument();
    setTheme('night');
    expect(theme.current).toBe('night');
    expect(dataset.theme).toBe('night');
    expect(kept.get('weavesignal.theme')).toBe('night');
    const { body } = render(App);
    expect(body).toMatch(/aria-pressed="true"[^>]*>Night</);
  });

  it('goes back to Dark by taking the attribute off', () => {
    const { dataset, kept } = fakeDocument();
    setTheme('night');
    setTheme('dark');
    expect('theme' in dataset).toBe(false);
    expect(kept.get('weavesignal.theme')).toBe('dark');
    applyTheme('light');
    expect(dataset.theme).toBe('light');
  });

  it('steps through all three in order', () => {
    fakeDocument();
    const seen = THEMES.map(() => (toggleTheme(), theme.current));
    expect(seen).toEqual(['light', 'night', 'dark']);
  });
});

// The tokens file as text. Vitest hands back an empty string for a stylesheet, ?raw or not, and
// the app has no Node types, so the module is named in a way the type checker does not follow.
const fsModule = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsModule)) as { readFileSync(path: URL, encoding: 'utf8'): string };
const css = readFileSync(new URL('../../../packages/ui/tokens.css', import.meta.url), 'utf8');

describe('the Night tokens (ADR-0270)', () => {
  const night = css.slice(css.indexOf(':root[data-theme="night"] {')).split('}')[0];

  it('names a ground, an ink, an accent and every live state', () => {
    for (const t of ['--plane', '--surface', '--ink', '--muted', '--accent', '--live-onair', '--live-cue', '--live-talk', '--live-open', '--mode-auto', '--cart-12'])
      expect(night, t).toContain(`${t}:`);
  });

  it('has no colour with a blue or cyan hue: blue is never above red', () => {
    const colours = [...night.matchAll(/#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})\b|rgba?\((\d+), (\d+), (\d+)/g)].map((m) =>
      m[1] ? [m[1], m[2], m[3]].map((h) => parseInt(h, 16)) : [m[4], m[5], m[6]].map(Number),
    );
    expect(colours.length).toBeGreaterThan(40);
    for (const [r, g, b] of colours) {
      expect(b, `rgb(${r}, ${g}, ${b})`).toBeLessThan(r);
      // Green may lead (talk, good, auto, the green carts) only with little blue in it: no teal, no cyan.
      if (g > r) expect(b, `rgb(${r}, ${g}, ${b})`).toBeLessThan(g * 0.7);
    }
  });
});
