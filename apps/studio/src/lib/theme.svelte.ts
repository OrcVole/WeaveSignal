// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The theme: dark by default, switched only by the person, never by the operating system
// (ADR-0065). The choice is remembered on this machine.

export type Theme = 'dark' | 'light';

const KEY = 'weavesignal.theme';

function stored(): Theme {
  try {
    return globalThis.localStorage?.getItem(KEY) === 'light' ? 'light' : 'dark';
  } catch {
    return 'dark';
  }
}

export const theme = $state({ current: stored() });

/** Put the theme on the document: dark is the root's own tokens, light sets data-theme. */
export function applyTheme(t: Theme): void {
  const root = globalThis.document?.documentElement;
  if (!root) return;
  if (t === 'light') root.dataset.theme = 'light';
  else delete root.dataset.theme;
}

export function toggleTheme(): void {
  setTheme(theme.current === 'dark' ? 'light' : 'dark');
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
