// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// A station's accent colour, kept legible (ADR-0237). A station chooses one colour; the public
// pages have two themes. The colour is never used as given: for each theme its lightness is
// moved, as little as possible, until
//
//   - it reads as link text on the theme's three grounds (4.5:1, which also clears the focus
//     ring's 3:1), and
//   - an ink reads on it, and on the primary button's hover colour, at 4.5:1.
//
// Hue and chroma are kept (OKLCH), so the station still sees its own colour. Only --accent and
// --accent-ink are ever set; every other token stays the design system's. The pages, the
// station's setup page and the website's wizard all use this one module; its test reads the
// grounds from packages/ui/tokens.css, so a change of tokens fails here first.

export type ThemeName = 'dark' | 'light';

/** The colours an accent has to live with in one theme (packages/ui/tokens.css). */
export interface Grounds {
  plane: string;
  surface: string;
  surface2: string;
  /** The theme's text colour: the primary button's hover mixes 12 % of it into the accent. */
  ink: string;
  /** The product's own accent and its ink, kept when a station sets none. */
  accent: string;
  accentInk: string;
}

export const GROUNDS: Record<ThemeName, Grounds> = {
  dark: { plane: '#13141c', surface: '#1a1b26', surface2: '#1f2130', ink: '#c0caf5', accent: '#7aa2f7', accentInk: '#13141c' },
  light: { plane: '#f4f5fa', surface: '#fcfcfb', surface2: '#eef0f6', ink: '#1a1b26', accent: '#0072cf', accentInk: '#ffffff' },
};

/** The inks tried on an accent, in order: the dark plane's colour, then white. */
export const INKS = ['#13141c', '#ffffff'] as const;

/** Text and link contrast (WCAG 2.1 AA, 1.4.3). */
export const TEXT_CONTRAST = 4.5;

type Rgb = [number, number, number];

/** `#rrggbb` or `#rgb`, with or without the hash, as lower-case `#rrggbb`; `null` if it is neither. */
export function normaliseHex(input: string): string | null {
  const s = input.trim().replace(/^#/, '').toLowerCase();
  if (/^[0-9a-f]{6}$/.test(s)) return `#${s}`;
  if (/^[0-9a-f]{3}$/.test(s)) return `#${s[0]}${s[0]}${s[1]}${s[1]}${s[2]}${s[2]}`;
  return null;
}

function rgbOf(hex: string): Rgb {
  const h = normaliseHex(hex);
  if (!h) throw new Error(`not a colour: ${hex}`);
  return [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255) as Rgb;
}

function hexOf(rgb: Rgb): string {
  return `#${rgb.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0')).join('')}`;
}

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGamma = (v: number) => (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055);

/** WCAG relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = rgbOf(hex).map(toLinear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio of two colours, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** `color-mix(in srgb, a <share>, b)`: the browsers' mix, on gamma-encoded values. */
export function mix(a: string, b: string, share: number): string {
  const [x, y] = [rgbOf(a), rgbOf(b)];
  return hexOf(x.map((v, i) => v * share + y[i] * (1 - share)) as Rgb);
}

// OKLab (Björn Ottosson, 2020), by its published matrices.
function oklabOf(hex: string): Rgb {
  const [r, g, b] = rgbOf(hex).map(toLinear);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

/** Linear sRGB of an OKLab colour; values outside 0 to 1 are outside the sRGB gamut. */
function linearOf([L, a, b]: Rgb): Rgb {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const inGamut = (rgb: Rgb) => rgb.every((v) => v >= -0.0005 && v <= 1.0005);

/** The sRGB colour at lightness `L` with this hue, and as much of `chroma` as sRGB holds there. */
function atLightness(L: number, chroma: number, hue: number): string {
  const make = (c: number): Rgb => linearOf([L, c * Math.cos(hue), c * Math.sin(hue)]);
  let c = chroma;
  if (!inGamut(make(c))) {
    let lo = 0;
    let hi = chroma;
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(make(mid))) lo = mid;
      else hi = mid;
    }
    c = lo;
  }
  return hexOf(make(c).map((v) => toGamma(Math.min(1, Math.max(0, v)))) as Rgb);
}

/** One theme's accent, derived from the station's colour. */
export interface Derived {
  /** `--accent` */
  accent: string;
  /** `--accent-ink` */
  ink: string;
  /** Whether the colour had to be moved from what the station chose. */
  adjusted: boolean;
  /** The lowest contrast of the accent as text on the theme's three grounds. */
  onGrounds: number;
  /** The lower contrast of the ink on the accent and on the button's hover colour. */
  inkOn: number;
}

function measure(accent: string, g: Grounds): { onGrounds: number; ink: string; inkOn: number } {
  const onGrounds = Math.min(contrast(accent, g.plane), contrast(accent, g.surface), contrast(accent, g.surface2));
  const hover = mix(accent, g.ink, 0.88);
  let best = { ink: INKS[0] as string, inkOn: 0 };
  for (const ink of INKS) {
    const inkOn = Math.min(contrast(ink, accent), contrast(ink, hover));
    if (inkOn > best.inkOn) best = { ink, inkOn };
  }
  return { onGrounds, ...best };
}

/**
 * The accent one theme shows for the station's colour, or `null` when the colour is not a
 * colour or (never seen; the test sweeps for it) no lightness of it is legible. `null` means
 * the theme keeps the product's own accent.
 */
export function derive(colour: string, theme: ThemeName): Derived | null {
  const hex = normaliseHex(colour);
  if (!hex) return null;
  const g = GROUNDS[theme];
  const [L0, a, b] = oklabOf(hex);
  const chroma = Math.hypot(a, b);
  const hue = Math.atan2(b, a);
  const ok = (m: { onGrounds: number; inkOn: number }) => m.onGrounds >= TEXT_CONTRAST && m.inkOn >= TEXT_CONTRAST;
  const given = measure(hex, g);
  if (ok(given)) return { accent: hex, adjusted: false, ...given };
  // Outwards from the chosen lightness, a 400th at a time: the nearest legible one wins. A dark
  // theme's accent is lighter than its grounds and a light theme's darker, so that side first.
  const first = theme === 'dark' ? 1 : -1;
  for (let step = 1; step <= 400; step++) {
    for (const dir of [first, -first]) {
      const L = L0 + (dir * step) / 400;
      if (L < 0 || L > 1) continue;
      const accent = atLightness(L, chroma, hue);
      const m = measure(accent, g);
      if (ok(m)) return { accent, adjusted: true, ...m };
    }
  }
  return null;
}

/** Both themes' accents for one colour. */
export function deriveBoth(colour: string): Record<ThemeName, Derived | null> {
  return { dark: derive(colour, 'dark'), light: derive(colour, 'light') };
}
