// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Panel names (ADR-0070): every panel carries `data-panel="surface.panel[.n]"`; hovering or
// focusing its title shows the plate, the name in monospace and one line of what the panel is.
// A port of the design pages' plate script (the same on all eight pages): the plate is fixed,
// placed over the panel's header row and shrunk or shifted until a pointer under it would hit
// no control, so it never covers a live control and never takes a key (it listens for Esc to
// hide itself and does not consume it). `?panel=all` pins every plate; `?audit=1` pins them all
// and lists any that lies over a control.
//
// A title that is a control with controls all round it (the step button in the frame, over the
// Carts tab's row of banks) has no room beside it. Its plate then goes along the row above or
// below to the first place that is clear, however far, before it would ever lie over the
// control it names (issue 27). Such a plate keeps clear of the plates already showing, and
// when every plate is pinned it is placed after the rest, so it is not put where another plate
// is about to go. Where no gap holds 160 px of it, it takes one that holds its name alone; and a
// pinned plate with no room even for that is left out, and the audit lists it as `no room`: a
// plate is never drawn over a control to make a screenshot complete. With nothing pinned,
// pointing at its title shows it as ever: alone, it has the room the pinned plates took.
//
// A plate is for a title a person can see. A pinned panel that is not drawn (the desk's chat
// glance when a toast leaves it no room, a closed dialog) has no plate, as a panel that is not
// on the page has none. It used to keep the plate where its title had last been, which by then
// was the cart wall (the M2 drive, 2026-11-17). A pinned panel that is drawn but scrolled or
// squeezed out of its column's sight has no plate either, and the audit lists it as `not in
// view`; its plate used to be held at the window's edge, over the status bar. The plates are
// placed again whenever a panel changes size, since a column can move with no node added.

import { fixedScale } from '../../../../packages/ui/fixed.ts';

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
    // A panel's titles are its own: one inside a panel nested in it (the step button in the
    // frame, a tile on the stage) is that panel's, or pointing at it would show two plates.
    // A panel with no title of its own keeps the titles inside it, as before: the packaged
    // app's frame is one (the station's name there is the Stations button, a panel itself).
    const all = [...p.querySelectorAll<HTMLElement>('[data-panel-title]')];
    const t = all.filter((x) => x.closest('[data-panel]') === p);
    if (t.length) return t;
    if (all.length) return all;
    const one = p.querySelector<HTMLElement>(TITLE);
    return [one ?? (p as HTMLElement)];
  };
  const headOf = (t: Element, p: Element): Element => {
    if (t === p) return p;
    const h = t.closest(HEAD);
    return h && p.contains(h) && h !== p ? h : h === p ? p : t;
  };
  /** Whether a person can see this title: it has a box, and at least half its height is left
   *  inside the window and inside every ancestor that cuts off what runs out of it. A dialog
   *  shown over the page, and anything fixed to the window, is cut by nothing outside itself. */
  const inView = (t: Element): boolean => {
    if (!t.getClientRects().length) return false;
    const tr = t.getBoundingClientRect();
    if (!tr.width && !tr.height) return false;
    if (win.getComputedStyle(t).visibility !== 'visible') return false;
    let { left, top, right, bottom } = tr;
    left = Math.max(left, 0);
    top = Math.max(top, 0);
    right = Math.min(right, win.innerWidth);
    bottom = Math.min(bottom, win.innerHeight);
    const free = (e: Element, cs: CSSStyleDeclaration) => cs.position === 'fixed' || e.matches('dialog:modal');
    for (let a = free(t, win.getComputedStyle(t)) ? null : t.parentElement; a && a !== doc.documentElement; a = a.parentElement) {
      const cs = win.getComputedStyle(a);
      const cutX = cs.overflowX !== 'visible';
      const cutY = cs.overflowY !== 'visible';
      if (cutX || cutY) {
        const b = a.getBoundingClientRect();
        if (cutX) {
          left = Math.max(left, b.left);
          right = Math.min(right, b.right);
        }
        if (cutY) {
          top = Math.max(top, b.top);
          bottom = Math.min(bottom, b.bottom);
        }
      }
      if (free(a, cs)) break;
    }
    return right - left >= Math.min(tr.width, 8) && bottom - top >= tr.height / 2;
  };
  /** The stretch of the window, top to bottom, that the ancestors of a title leave in sight: the
   *  column it scrolls in. Unbounded where nothing above it cuts off what runs out. */
  const column = (t: Element): { top: number; bottom: number } => {
    let top = -Infinity;
    let bottom = Infinity;
    const free = (e: Element, cs: CSSStyleDeclaration) => cs.position === 'fixed' || e.matches('dialog:modal');
    for (let a = free(t, win.getComputedStyle(t)) ? null : t.parentElement; a && a !== doc.documentElement; a = a.parentElement) {
      const cs = win.getComputedStyle(a);
      if (cs.overflowY !== 'visible') {
        const b = a.getBoundingClientRect();
        top = Math.max(top, b.top);
        bottom = Math.min(bottom, b.bottom);
      }
      if (free(a, cs)) break;
    }
    return { top, bottom };
  };
  /** The pinned plates placed in this pass (`replace`): a name drawn twice shares one plate, and
   *  a second title that has no place in sight must not take away the first one's. */
  let placedNow = new Set<HTMLElement>();
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
  /** The first clear place along a row, from `left` rightwards, for a plate up to `w` wide:
   *  the first gap between the controls and the other plates showing that holds `need` of it
   *  (160 px, or the whole plate if it is shorter; at the last, its name alone). Null when the
   *  row has none. */
  const along = (left: number, top: number, w: number, h: number, self: HTMLElement, need = Math.min(w, 160)) => {
    const edge = win.innerWidth - 4;
    const band = { left, top, right: edge, bottom: top + h };
    const plates = [...doc.querySelectorAll<HTMLElement>('.pn-tag.show')].filter((x) => x !== self).map((x) => x.getBoundingClientRect());
    const blocks = [...controlsAt(band).map((c) => c.getBoundingClientRect()), ...plates]
      .filter((b) => b.right > left && b.left < edge && b.top < band.bottom && band.top < b.bottom)
      .sort((a, b) => a.left - b.left);
    let x = left;
    for (const b of blocks) {
      if (b.left - 6 - x >= need) break;
      x = Math.max(x, b.right + 6);
    }
    const next = blocks.find((b) => b.left >= x);
    const right = Math.min(x + w, next ? next.left - 6 : edge, edge);
    return right - x >= need ? { left: x, top, right, bottom: top + h } : null;
  };
  /** Place a title's plate. With `later`, a plate that finds no room by its title is left out
   *  and the answer is false: the caller places it after the others (`far`). */
  const show = (p: Element, t: HTMLElement, tag: HTMLElement, later = false, far = false): boolean => {
    if (!inView(t)) return true;
    const tr = t.getBoundingClientRect();
    const hr = headOf(t, p).getBoundingClientRect();
    tag.classList.add('show');
    tag.style.maxWidth = 'none';
    // Everything below is in viewport pixels, as rects are; the plate's style is in CSS pixels,
    // which the studio's text size (zoom on <html>) scales, so it is divided by `zs` on the way out.
    const zs = fixedScale(tag);
    tag.style.left = '-9999px';
    const pr = p.getBoundingClientRect();
    const rem = (parseFloat(win.getComputedStyle(doc.documentElement).fontSize) || 16) * zs;
    const size = tag.getBoundingClientRect();
    // The name alone, with the plate's own padding and border: the least a plate can be.
    const code = tag.firstElementChild?.getBoundingClientRect();
    const nameW = code ? Math.min(size.width, code.width + 2 * (code.left - size.left)) : size.width;
    const th = size.height;
    delete tag.dataset.noRoom;
    let tw = Math.min(size.width, 30 * rem, win.innerWidth - 8);
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
    for (const [cx, cy] of far ? [] : cands) {
      if (best) break;
      const l = Math.max(4, cx);
      const y = Math.max(0, Math.min(cy, win.innerHeight - th));
      const r = fit(l, y, Math.min(tw, win.innerWidth - 4 - l), th);
      if (r && r.right - r.left >= Math.min(tw, 160)) best = r;
      else if (r && !best) best = r;
    }
    // No room by the title: the nearest clear place along the same rows, before the plate lies
    // over a control.
    const resort = { left: Math.max(4, tr.left), top: Math.max(0, rowTop), right: Math.max(4, tr.left) + Math.min(tw, 160) };
    // Only a plate that would lie over a control looks further: where the place by the title
    // happens to be clear, the plate stays by its title.
    if (!best && controlsAt({ ...resort, bottom: resort.top + th }).length) {
      if (later) {
        tag.classList.remove('show');
        return false;
      }
      // Not under the plate itself: it is drawn, off to the left, to be measured.
      tag.classList.remove('show');
      for (const need of [Math.min(tw, 160), Math.min(tw, nameW)]) {
        if (best) break;
        for (const [cx, cy] of cands) {
          const y = Math.max(0, Math.min(cy, win.innerHeight - th));
          const r = along(Math.max(4, cx), y, tw, th, tag, need);
          if (r && (!best || r.left - tr.left < best.left - tr.left)) best = r;
        }
      }
      // Pinned with the rest and no room anywhere along its rows: left out, never over a control.
      if (!best && far) {
        tag.dataset.noRoom = '1';
        return true;
      }
      tag.classList.add('show');
    }
    best ??= resort;
    // A pinned plate is fixed to the window, not to the column its title scrolls in: one whose
    // place falls below what the column shows (a header half under the column's foot, its plate
    // a few pixels lower still) would be drawn over the row under the column. Such a title is
    // not in view: no plate, and the audit says so. A plate just above the column's top, over
    // a title at its head, stays.
    if (pinned(p.getAttribute('data-panel') ?? '')) {
      const col = column(t);
      if (best.top + th > col.bottom + 1 || best.top < col.top - th - 6) {
        if (!placedNow.has(tag)) {
          tag.classList.remove('show');
          tag.dataset.away = '1';
        }
        return true;
      }
      placedNow.add(tag);
      delete tag.dataset.away;
    }
    tag.style.left = `${best.left / zs}px`;
    tag.style.top = `${best.top / zs}px`;
    tag.style.maxWidth = `${(best.right - best.left) / zs}px`;
    return true;
  };
  const hide = (tag: HTMLElement) => tag.classList.remove('show');
  const pinned = (name: string) => all || pin.includes(name);
  // A panel that changes size moves the ones after it, with no node added or removed (a line of
  // text that arrives, a class that changes a column's rows): the pinned plates follow. Only a
  // window that pins plates watches for it; with none pinned there is nothing to place again.
  const sizes = (all || pin.length) && typeof win.ResizeObserver === 'function' ? new win.ResizeObserver(() => schedule()) : null;

  const bind = () => {
    for (const p of doc.querySelectorAll('[data-panel]')) {
      if (bound.has(p)) continue;
      bound.add(p);
      sizes?.observe(p);
      const name = p.getAttribute('data-panel')!;
      const tag = plate(name);
      for (const t of titles(p)) {
        if (!t.hasAttribute('tabindex') && !t.matches(CONTROL)) t.setAttribute('tabindex', '0');
        // Merge, never overwrite: a state chip or lamp names its own tooltip there too.
        const ids = (t.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean);
        if (!ids.includes(tag.id)) t.setAttribute('aria-describedby', [...ids, tag.id].join(' '));
        // A pinned plate that was left out for want of room stays out: pointing at its title
        // must not draw it over the control it had no room beside.
        const on = () => {
          if (!(pinned(name) && tag.dataset.noRoom)) show(p, t, tag);
        };
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
    // The plates with no room by their titles go last, clear of the ones that have.
    const last: [Element, HTMLElement, HTMLElement][] = [];
    placedNow = new Set();
    for (const tag of doc.querySelectorAll<HTMLElement>('.pn-tag[data-no-room]')) delete tag.dataset.noRoom;
    // Each pinned panel's titles that can be seen. A name drawn twice shares one plate (the
    // lamp, a toast slot), so a name is out of view only when every title under it is.
    const panels = [...doc.querySelectorAll('[data-panel]')].map((p) => {
      const name = p.getAttribute('data-panel')!;
      const mine = pinned(name) ? titles(p) : [];
      return { p, name, seen: mine.filter(inView), drawn: mine.some((t) => t.getClientRects().length > 0) };
    });
    const inSight = new Set(panels.filter((x) => x.seen.length).map((x) => x.name));
    const drawn = new Set(panels.filter((x) => x.drawn).map((x) => x.name));
    // First the plates that go: one left where its title used to be would lie over whatever
    // is there now, and the plates still to be placed would keep clear of it.
    for (const { name } of panels) {
      const tag = plate(name);
      const gone = pinned(name) && !inSight.has(name);
      if (gone && drawn.has(name)) tag.dataset.away = '1';
      else delete tag.dataset.away;
      if (gone || !pinned(name)) hide(tag);
    }
    for (const { p, name, seen } of panels) {
      // A panel with several titles has one plate, and the last title's place is the one it
      // keeps: it waits only when that one has no room.
      let waits: [Element, HTMLElement, HTMLElement] | null = null;
      for (const t of seen) waits = show(p, t, plate(name), true) ? null : [p, t, plate(name)];
      if (waits) last.push(waits);
    }
    for (const [p, t, tag] of last) show(p, t, tag, false, true);
    // A plate whose panel left the page goes with it.
    for (const tag of doc.querySelectorAll<HTMLElement>('.pn-tag.show, .pn-tag[data-away]')) {
      const name = tag.querySelector('code')?.textContent ?? '';
      if (!doc.querySelector(`[data-panel="${name}"]`)) {
        hide(tag);
        delete tag.dataset.away;
      }
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
    // A pinned plate that found no room is not drawn; it is listed, so the count stays honest.
    for (const tag of doc.querySelectorAll<HTMLElement>('.pn-tag[data-no-room]:not(.show)')) out.push(`${tag.firstChild?.textContent}  no room`);
    // So is a pinned panel that is drawn where its title cannot be seen: out of its column's sight.
    for (const tag of doc.querySelectorAll<HTMLElement>('.pn-tag[data-away]:not(.show)')) out.push(`${tag.firstChild?.textContent}  not in view`);
    let pre = doc.getElementById('pn-audit');
    if (!pre) {
      pre = doc.createElement('pre');
      pre.id = 'pn-audit';
      pre.style.cssText =
        'position:fixed;left:8px;bottom:8px;z-index:99;max-height:calc(var(--win-h, 100vh) * 0.6);overflow:auto;background:var(--plane);color:var(--ink);font:600 13px/1.35 monospace;padding:8px 10px;border-radius:6px;opacity:.95;margin:0';
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
      sizes?.disconnect();
      win.removeEventListener('scroll', replace, { capture: true });
      win.removeEventListener('resize', replace);
      all = false;
    },
  };
}
