// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The theme: dark by default, switched only by the person, never by the operating system
// (ADR-0065). Night is the third: warm, with no blue, for presenting late (ADR-0270). The choice
// is remembered on this machine.

export const THEMES = ['dark', 'light', 'night'] as const;
export type Theme = (typeof THEMES)[number];

/** The word on each theme's button. */
export const THEME_WORDS: Record<Theme, string> = { dark: 'Dark', light: 'Light', night: 'Night' };

const KEY = 'weavesignal.theme';

function stored(): Theme {
  try {
    const kept = globalThis.localStorage?.getItem(KEY);
    return THEMES.find((t) => t === kept) ?? 'dark';
  } catch {
    return 'dark';
  }
}

export const theme = $state({ current: stored() });

/** Put the theme on the document: dark is the root's own tokens, light and night set data-theme. */
export function applyTheme(t: Theme): void {
  const root = globalThis.document?.documentElement;
  if (!root) return;
  if (t === 'dark') delete root.dataset.theme;
  else root.dataset.theme = t;
}

export function toggleTheme(): void {
  setTheme(THEMES[(THEMES.indexOf(theme.current) + 1) % THEMES.length]);
}

/** The person chose a theme: show it and remember it on this machine. */
export function setTheme(t: Theme): void {
  theme.current = t;
  applyTheme(theme.current);
  try {
    globalThis.localStorage?.setItem(KEY, theme.current);
  } catch {
    // Storage refused: the choice lasts for this session.
  }
}
