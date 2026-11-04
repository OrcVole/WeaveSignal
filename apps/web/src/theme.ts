// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The public pages follow the system's light or dark setting. ADR-0065's explicit toggle is the
// studio app's (a desk must not change at sunset mid-show) and leaves the listener-facing pages
// open; a listener's phone already says which they prefer. Dark is the tokens' own default.
import '../../../packages/ui/tokens.css';
import './web.css';

export function followSystemTheme(): void {
  const root = document.documentElement;
  const q = globalThis.matchMedia?.('(prefers-color-scheme: light)');
  const apply = () => {
    if (q?.matches) root.dataset.theme = 'light';
    else delete root.dataset.theme;
  };
  apply();
  q?.addEventListener?.('change', apply);
}
