// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contrast, derive, deriveBoth, GROUNDS, INKS, mix, normaliseHex, TEXT_CONTRAST, type ThemeName } from './accent.ts';

const css = readFileSync(new URL('../ui/tokens.css', import.meta.url), 'utf8');
/** One theme's block of packages/ui/tokens.css: `:root {…}` is dark, `[data-theme="light"]` light. */
function block(theme: ThemeName): string {
  const start = theme === 'dark' ? css.indexOf(':root {') : css.indexOf(':root[data-theme="light"] {');
  expect(start).toBeGreaterThanOrEqual(0);
  return css.slice(start, css.indexOf('\n}', start));
}
function token(theme: ThemeName, name: string): string {
  const m = new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})\\b`).exec(block(theme));
  if (!m) throw new Error(`--${name} is not a hex colour in the ${theme} tokens`);
  return m[1].toLowerCase();
}

describe('the grounds an accent is derived against', () => {
  it.each(['dark', 'light'] as const)('are the %s tokens of packages/ui/tokens.css', (theme) => {
    expect(GROUNDS[theme]).toEqual({
      plane: token(theme, 'plane'),
      surface: token(theme, 'surface'),
      surface2: token(theme, 'surface-2'),
      ink: token(theme, 'ink'),
      accent: token(theme, 'accent'),
      accentInk: token(theme, 'accent-ink'),
    });
  });
  it('uses the accent only where the derivation checks it', () => {
    // web.css: link text and glyphs on the grounds, the primary button's fill with its ink, the
    // hover mix at 88 %, the focus ring, the checkbox. A new use needs a look here.
    const web = readFileSync(new URL('../../apps/web/src/web.css', import.meta.url), 'utf8');
    const uses = web.match(/[^;{}]*var\(--accent\)[^;{}]*/g)!.map((u) => u.trim());
    expect(uses.sort()).toEqual(
      [
        'accent-color: var(--accent)',
        'background: color-mix(in srgb, var(--accent) 88%, var(--ink))',
        'background: var(--accent)',
        'color: var(--accent)',
        'color: var(--accent)',
        'color: var(--accent)',
      ].sort(),
    );
    expect(css).toContain('--focus-ring: 2px solid var(--accent)');
  });
});

describe('contrast', () => {
  it('is 21 for black on white and 1 for a colour on itself', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrast('#7aa2f7', '#7aa2f7')).toBe(1);
  });
  it('mixes as color-mix(in srgb) does', () => {
    expect(mix('#ffffff', '#000000', 0.5)).toBe('#808080');
    expect(mix('#ff0000', '#0000ff', 0.88)).toBe('#e0001f');
  });
});

describe('hex colours', () => {
  it('takes six or three digits, with or without the hash', () => {
    expect(normaliseHex('#B4532A')).toBe('#b4532a');
    expect(normaliseHex('b4532a')).toBe('#b4532a');
    expect(normaliseHex(' #fa0 ')).toBe('#ffaa00');
  });
  it.each(['', 'red', '#12345', '#gggggg', 'rgb(1,2,3)', '#1234567', 'url(x)'])('refuses %j', (v) => {
    expect(normaliseHex(v)).toBeNull();
    expect(derive(v, 'dark')).toBeNull();
  });
});

/** Every pair the pages use an accent in, for one theme. */
function pairs(theme: ThemeName, accent: string, ink: string) {
  const g = GROUNDS[theme];
  return {
    'link on the plane': contrast(accent, g.plane),
    'link on a card': contrast(accent, g.surface),
    'link in the consent box': contrast(accent, g.surface2),
    'ink on the button': contrast(ink, accent),
    'ink on the hovered button': contrast(ink, mix(accent, g.ink, 0.88)),
  };
}

// The hard ones first: colours with no legible lightness of their own in one theme or the other.
const HARD = ['#ffff00', '#000000', '#ffffff', '#808080', '#ff0000', '#00ff00', '#0000ff', '#ff00ff', '#00ffff', '#7f7f00', '#fa0', '#010101', '#fefefe'];
const SWEEP: string[] = [];
for (let r = 0; r <= 255; r += 51) for (let g = 0; g <= 255; g += 51) for (let b = 0; b <= 255; b += 51) SWEEP.push(`#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`);

describe('a derived accent', () => {
  it.each(['dark', 'light'] as const)('is legible in every pair, for every colour, in the %s theme', (theme) => {
    for (const colour of [...HARD, ...SWEEP]) {
      const d = derive(colour, theme);
      expect(d, `${colour} in ${theme}`).not.toBeNull();
      for (const [what, ratio] of Object.entries(pairs(theme, d!.accent, d!.ink))) {
        expect(ratio, `${colour} in ${theme}: ${what} (${d!.accent} with ${d!.ink})`).toBeGreaterThanOrEqual(TEXT_CONTRAST);
      }
      // The focus ring and the checkbox need 3:1 against the grounds (WCAG 1.4.11): 4.5 covers it.
      expect(d!.onGrounds).toBeGreaterThanOrEqual(3);
      expect(INKS).toContain(d!.ink);
      expect(d!.accent).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
  it('is the colour itself when the colour already reads', () => {
    const d = derive('#7aa2f7', 'dark')!;
    expect(d).toMatchObject({ accent: '#7aa2f7', ink: '#13141c', adjusted: false });
  });
  it('keeps the hue and moves only as far as it must', () => {
    // A deep red on the dark theme is lightened, and stays a red.
    const d = derive('#8b1a1a', 'dark')!;
    expect(d.adjusted).toBe(true);
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(d.accent.slice(i, i + 2), 16));
    expect(r).toBeGreaterThan(g);
    expect(r).toBeGreaterThan(b);
    expect(d.onGrounds).toBeLessThan(TEXT_CONTRAST + 0.35);
  });
  it('gives each theme its own', () => {
    const both = deriveBoth('#ffcc00');
    expect(both.dark!.adjusted).toBe(false);
    expect(both.light!.adjusted).toBe(true);
    expect(both.light!.ink).toBe('#ffffff');
    expect(both.dark!.ink).toBe('#13141c');
  });
});
