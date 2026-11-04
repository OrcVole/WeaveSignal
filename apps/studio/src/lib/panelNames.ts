// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Panel names (ADR-0070): every panel carries `data-panel="surface.panel[.n]"`; hovering or
// focusing its title shows the plate, the name in monospace and one line of what the panel is.
// A port of the design pages' plate script (the same on all eight pages): the plate is fixed,
// placed over the panel's header row and shrunk or shifted until a pointer under it would hit
// no control, so it never covers a live control and never takes a key (it listens for Esc to
// hide itself and does not consume it). `?panel=all` pins every plate; `?audit=1` pins them all
// and lists any that lies over a control.

const TITLE =
  ':scope > .ph > h2, :scope > .ph > h3, :scope > .head h2, :scope > .head h3, :scope > .head .name, :scope > .wh > span, :scope > h3, :scope > h2, h1, h2, h3';
const HEAD = '.ph, .head, .stage-head, .frame, .wh, .lamp, header';
const CONTROL = 'button, a[href], input, select, textarea, [role=tab], [role=slider]';

function id(name: string): string {
  return 'pn-' + name.replace(/[^a-z0-9]+/g, '-');
}

export interface PanelNames {
  refresh(): void;
  destroy(): void;
}

export function installPanelNames(doc: Document, registry: Readonly<Record<string, string>>): PanelNames {
  const win = doc.defaultView!;
  const q = new URLSearchParams(win.location.search);
  const pin = (q.get('panel') ?? '').split(',').filter(Boolean);
  const audit = q.get('audit') === '1';
  let all = pin.includes('all') || audit;
  const bound = new WeakSet<Element>();

  const titles = (p: Element): HTMLElement[] => {
    const t = p.querySelectorAll<HTMLElement>('[data-panel-title]');
    if (t.length) return [...t];
    const one = p.querySelector<HTMLElement>(TITLE);
    return [one ?? (p as HTMLElement)];
  };
  const headOf = (t: Element, p: Element): Element => {
    if (t === p) return p;
    const h = t.closest(HEAD);
    return h && p.contains(h) && h !== p ? h : h === p ? p : t;
  };
  const plate = (name: string): HTMLElement => {
    let tag = doc.getElementById(id(name));
    if (tag) return tag;
    tag = doc.createElement('div');
    tag.className = 'pn-tag';
    tag.id = id(name);
    tag.setAttribute('role', 'tooltip');
    const c = doc.createElement('code');
    c.textContent = name;
    const s = doc.createElement('span');
    s.textContent = registry[name] ?? '';
    tag.append(c, s);
    doc.body.appendChild(tag);
    return tag;
  };
  const controlsAt = (r: { left: number; top: number; right: number; bottom: number }): Element[] => {
    const hits: Element[] = [];
    for (let y = r.top + 3; y < r.bottom - 2; y += 7) {
      for (let x = r.left + 3; x < r.right - 2; x += 10) {
        const el = doc.elementFromPoint(x, y);
        const c = el?.closest(CONTROL + ', .cart');
        if (c && !hits.includes(c)) hits.push(c);
      }
    }
    return hits;
  };
  const fit = (left: number, top: number, w: number, h: number) => {
    for (let i = 0; i < 4; i++) {
      const r = { left, top, right: left + w, bottom: top + h };
      const hits = controlsAt(r);
      if (!hits.length) return r;
      let cut = Infinity;
      let push = -Infinity;
      for (const c of hits) {
        const b = c.getBoundingClientRect();
        if (b.left > left + 72) cut = Math.min(cut, b.left - 6);
        else push = Math.max(push, b.right + 6);
      }
      if (cut < Infinity) w = cut - left;
      else if (push > -Infinity) {
        w -= push - left;
        left = push;
      } else return null;
      if (w < 72) return null;
    }
    return null;
  };
  const show = (p: Element, t: HTMLElement, tag: HTMLElement) => {
    const tr = t.getBoundingClientRect();
    if (!tr.width && !tr.height) return;
    const hr = headOf(t, p).getBoundingClientRect();
    tag.classList.add('show');
    tag.style.maxWidth = 'none';
    tag.style.left = '-9999px';
    const pr = p.getBoundingClientRect();
    const rem = parseFloat(win.getComputedStyle(doc.documentElement).fontSize) || 16;
    const th = tag.offsetHeight;
    let tw = Math.min(tag.offsetWidth, 30 * rem, win.innerWidth - 8);
    if (pr.width && pr.right - Math.max(4, tr.left) >= 10 * rem) tw = Math.min(tw, pr.right - Math.max(4, tr.left));
    // A title that is, or holds, a state chip with its own tooltip (StateChip) starts above, so the
    // plate never covers the state word the tooltip explains.
    const isCtl = !!(t.matches(CONTROL) || t.closest(CONTROL) || t.matches('.st-trigger') || t.querySelector('.st-trigger'));
    const rowTop = hr.height > 3 * th ? tr.top + (tr.height - th) / 2 : hr.top + (hr.height - th) / 2;
    const row = (t.parentElement ?? t).getBoundingClientRect();
    const above = Math.min(tr.top, row.top) - th - 4;
    const below = Math.max(tr.bottom, row.bottom) + 4;
    const cands = isCtl
      ? [[tr.left, above], [tr.left, below], [tr.left, rowTop]]
      : [[tr.left, rowTop], [tr.left, above], [tr.left, below]];
    let best: { left: number; top: number; right: number } | null = null;
    for (const [cx, cy] of cands) {
      if (best) break;
      const l = Math.max(4, cx);
      const y = Math.max(0, Math.min(cy, win.innerHeight - th));
      const r = fit(l, y, Math.min(tw, win.innerWidth - 4 - l), th);
      if (r && r.right - r.left >= Math.min(tw, 160)) best = r;
      else if (r && !best) best = r;
    }
    best ??= { left: Math.max(4, tr.left), top: Math.max(0, rowTop), right: Math.max(4, tr.left) + Math.min(tw, 160) };
    tag.style.left = `${best.left}px`;
    tag.style.top = `${best.top}px`;
    tag.style.maxWidth = `${best.right - best.left}px`;
  };
  const hide = (tag: HTMLElement) => tag.classList.remove('show');
  const pinned = (name: string) => all || pin.includes(name);

  const bind = () => {
    for (const p of doc.querySelectorAll('[data-panel]')) {
      if (bound.has(p)) continue;
      bound.add(p);
      const name = p.getAttribute('data-panel')!;
      const tag = plate(name);
      for (const t of titles(p)) {
        if (!t.hasAttribute('tabindex') && !t.matches(CONTROL)) t.setAttribute('tabindex', '0');
        // Merge, never overwrite: a state chip or lamp names its own tooltip there too.
        const ids = (t.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean);
        if (!ids.includes(tag.id)) t.setAttribute('aria-describedby', [...ids, tag.id].join(' '));
        const on = () => show(p, t, tag);
        const off = () => {
          if (!pinned(name)) hide(tag);
        };
        t.addEventListener('mouseenter', on);
        t.addEventListener('mouseleave', off);
        t.addEventListener('focus', on);
        t.addEventListener('blur', off);
        t.addEventListener('keydown', (e) => {
          if (e.key === 'Escape' && tag.classList.contains('show') && !pinned(name)) hide(tag);
        });
      }
    }
  };
  const replace = () => {
    for (const p of doc.querySelectorAll('[data-panel]')) {
      const name = p.getAttribute('data-panel')!;
      if (pinned(name) && (p as HTMLElement).offsetParent !== null) titles(p).forEach((t) => show(p, t, plate(name)));
      else if (!pinned(name)) hide(plate(name));
    }
    // A plate whose panel left the page goes with it.
    for (const tag of doc.querySelectorAll<HTMLElement>('.pn-tag.show')) {
      const name = tag.querySelector('code')?.textContent ?? '';
      if (!doc.querySelector(`[data-panel="${name}"]`)) hide(tag);
    }
    if (audit) writeAudit();
  };
  const writeAudit = () => {
    const out: string[] = [];
    const tags = doc.querySelectorAll<HTMLElement>('.pn-tag.show');
    for (const tag of tags) {
      const hits = controlsAt(tag.getBoundingClientRect()).map(
        (c) => `${(c.className || c.tagName).toString().split(' ')[0]}:${(c.textContent ?? '').trim().slice(0, 16)}`,
      );
      out.push(`${tag.firstChild?.textContent}${hits.length ? '  OVER ' + hits.join(', ') : '  clear'}`);
    }
    let pre = doc.getElementById('pn-audit');
    if (!pre) {
      pre = doc.createElement('pre');
      pre.id = 'pn-audit';
      pre.style.cssText =
        'position:fixed;left:8px;bottom:8px;z-index:99;max-height:60vh;overflow:auto;background:#000;color:#fff;font:600 13px/1.35 monospace;padding:8px 10px;border-radius:6px;opacity:.95;margin:0';
      doc.body.appendChild(pre);
    }
    pre.textContent = `plates ${tags.length}\n${out.join('\n')}`;
  };

  let frame = 0;
  const schedule = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      bind();
      if (all || pin.length) replace();
    });
  };
  // The plates and the audit list are the observer's own writes: they never reschedule it.
  const ours = (n: Node) => n instanceof Element && (n.id === 'pn-audit' || n.classList.contains('pn-tag'));
  const observer = new MutationObserver((records) => {
    if (records.some((r) => !ours(r.target) && ![...r.addedNodes].every(ours))) schedule();
  });
  observer.observe(doc.body, { childList: true, subtree: true });
  win.addEventListener('scroll', replace, { passive: true, capture: true });
  win.addEventListener('resize', replace);
  schedule();
  return {
    refresh: schedule,
    destroy() {
      observer.disconnect();
      win.removeEventListener('scroll', replace, { capture: true });
      win.removeEventListener('resize', replace);
      all = false;
    },
  };
}
