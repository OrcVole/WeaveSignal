// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The station's look on its public pages (ADR-0237): an accent colour, a brand icon (also the
// favicon) and a banner. The pages ask `/api/v1/branding` once at load and remember the answer
// in the browser, so a return visit is branded from its first frame.
//
// A station that has set nothing answers with three nulls, and one without the route (an older
// station, the sandbox build) with 404: either way nothing here changes anything, and the
// pages look as they always did.
//
// The accent is never used as the station gave it: each theme's is derived so that it stays
// legible (packages/wizard/accent.ts), and only --accent and --accent-ink are set. They are
// set through the style object, which the pages' Content-Security-Policy allows; pictures come
// only from this station's own branding routes.
import { derive, normaliseHex, type ThemeName } from '../../../../packages/wizard/accent.ts';

export interface Picture {
  url: string;
  width: number;
  height: number;
}
export interface Look {
  accent: string | null;
  icon: Picture | null;
  banner: Picture | null;
}

/** What the pages' headers show: the station's pictures, when it has set them. */
export const brand = $state<{ icon: Picture | null; banner: Picture | null }>({ icon: null, banner: null });

const KEPT = 'weavesignal.look.v1';
const NONE: Look = { accent: null, icon: null, banner: null };

/** A picture as the station's own route gives it, or null: nothing else is ever shown. */
function picture(v: unknown, kind: 'icon' | 'banner'): Picture | null {
  const p = v as Partial<Picture> | null;
  if (!p || typeof p.url !== 'string' || !/^\/api\/v1\/branding\/(icon|banner)\?v=[0-9a-f]{16}$/.test(p.url)) return null;
  if (!p.url.startsWith(`/api/v1/branding/${kind}?`)) return null;
  const size = (n: unknown) => (typeof n === 'number' && Number.isFinite(n) && n > 0 && n <= 8192 ? Math.round(n) : 0);
  const [width, height] = [size(p.width), size(p.height)];
  return width && height ? { url: p.url, width, height } : null;
}

/** An answer from the station or from the browser's memory, checked. */
export function lookOf(v: unknown): Look {
  if (typeof v !== 'object' || v === null) return NONE;
  const o = v as Record<string, unknown>;
  return {
    accent: typeof o.accent === 'string' ? normaliseHex(o.accent) : null,
    icon: picture(o.icon, 'icon'),
    banner: picture(o.banner, 'banner'),
  };
}

const theme = (): ThemeName => (document.documentElement.dataset.theme === 'light' ? 'light' : 'dark');

let accent: string | null = null;
function paint() {
  const style = document.documentElement.style;
  const d = accent ? derive(accent, theme()) : null;
  if (d) {
    style.setProperty('--accent', d.accent);
    style.setProperty('--accent-ink', d.ink);
  } else {
    style.removeProperty('--accent');
    style.removeProperty('--accent-ink');
  }
}

let ownIcon: { href: string; type: string } | null = null;
function favicon(icon: Picture | null) {
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (!link) return;
  ownIcon ??= { href: link.getAttribute('href') ?? '', type: link.getAttribute('type') ?? '' };
  link.setAttribute('href', icon ? icon.url : ownIcon.href);
  link.setAttribute('type', icon ? 'image/png' : ownIcon.type);
  let touch = document.querySelector<HTMLLinkElement>('link[rel="apple-touch-icon"]');
  if (icon) {
    if (!touch) {
      touch = document.createElement('link');
      touch.rel = 'apple-touch-icon';
      document.head.append(touch);
    }
    touch.href = icon.url;
  } else {
    touch?.remove();
  }
}

/** Put a look on the page. */
export function apply(look: Look) {
  accent = look.accent;
  paint();
  brand.icon = look.icon;
  brand.banner = look.banner;
  favicon(look.icon);
}

function remembered(): Look {
  try {
    return lookOf(JSON.parse(localStorage.getItem(KEPT) ?? 'null'));
  } catch {
    return NONE;
  }
}
function remember(look: Look) {
  try {
    if (look.accent || look.icon || look.banner) localStorage.setItem(KEPT, JSON.stringify(look));
    else localStorage.removeItem(KEPT);
  } catch {
    // a private window, or storage turned off: the look is asked for again next time
  }
}

/** Ask the station for its look; `null` when it has no such route or does not answer. */
export async function fetchLook(): Promise<Look | null> {
  try {
    const res = await fetch('/api/v1/branding', { headers: { Accept: 'application/json' } });
    if (!res.ok) return null;
    return lookOf(await res.json());
  } catch {
    return null;
  }
}

/**
 * Brand this page: at once from what the browser remembers, then from the station. Call it
 * after the theme is set; it follows the theme from then on.
 */
export function startBrand(): void {
  const kept = remembered();
  if (kept.accent || kept.icon || kept.banner) apply(kept);
  new MutationObserver(paint).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  void fetchLook().then((look) => {
    // No answer keeps what the browser remembers; an answer, even an empty one, replaces it.
    if (!look) return;
    apply(look);
    remember(look);
  });
}
