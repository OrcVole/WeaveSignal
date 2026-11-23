// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The handover window at the desk (1.2; handover-window.md): the stretch before a show's post in
// which the desk says who is next, from where, and whether they are ready. The station works the
// window out (`post.window` in the view, crates/model/src/handover_window.rs); this only puts it
// into the desk's words, as `post.ts` does for the post's own rule. The window changes no act:
// the grace, the late count and Hold keep the line above, and the handover's key (⇧⏎) is as it
// was.
//
// The rows sit under the clock's post line (`desk.clock`), at most two. Usually there is one:
// the next host. There are two when the show under way still waits for a host who never came
// and the next show's window has opened: the station then moves the window to the next show
// and the desk says both, the late one first, so that neither host is lost from sight. From
// 1.2.1 the station names the late host itself (`post.window.late`): a presenter who never came
// to the room has no source for the post to name, and was left out. A 1.2.0 station sends no
// `late`, and the desk reads it from the post as before (`post.window.show` differs from
// `post.show.slug`).

import type { HandoverWindow, HostOrigin, Notice, PostState, PostView, Source, View } from './api.ts';
import { firstName, hm } from './format.ts';
import { CHIP, type Chip } from '../../../../packages/ui/states.ts';

/** How far a host has got, in the three words the window uses. */
export type Readiness = 'not_here' | 'connecting' | 'ready';

export interface WindowRow {
  /** `late`: the show under way still waits for them. `next`: who the window announces.
   *  `overrun`: the post has come and the outgoing host still presents. */
  is: 'late' | 'next' | 'overrun';
  /** The row's own word, in small capitals before the name: *Next*, *Late*, *Over*. */
  label: string;
  /** Who: the host down for the show, or for `overrun` whoever is running over. */
  who: string;
  /** The show and its start on the station's clock: *Kettle Drive · 14:00*. */
  what: string;
  /** The show's slug, so the desk can leave `what` out under a post line that names it. */
  show: string;
  /** *own studio* or *in the room*; null where the station did not say. */
  from: string | null;
  /** The quiet end of the second line: a heads-up given, a hold, who waits. */
  note: string | null;
  readiness: Readiness | null;
  /** The whole row as one sentence, for a screen reader and the row's tooltip. */
  said: string;
}

const READY_WORD: Record<Readiness, string> = { not_here: 'not here yet', connecting: 'connecting', ready: 'ready' };

/** The state chip for a host's readiness: the vocabulary's own chips with the window's words,
 *  so a mark and a word carry it, never the colour alone. */
export function readinessChip(r: Readiness): { chip: Chip; word: string } {
  if (r === 'ready') return { chip: CHIP.ready, word: 'Ready' };
  if (r === 'connecting') return { chip: CHIP.waiting, word: 'Connecting' };
  return { chip: CHIP.notConnected, word: 'Not here yet' };
}

const ofPost = (s: PostState | null | undefined): Readiness | null =>
  s === 'standing_by' ? 'ready' : s === 'connecting' ? 'connecting' : s === 'not_connected' || s === 'link_lost' ? 'not_here' : null;

const ofWindow = (w: HandoverWindow): Readiness | null => (w.state === 'ready' ? 'ready' : w.state === 'cueing' ? 'connecting' : w.state === 'announced' ? 'not_here' : null);

/** Where a host comes from, as the rundown's row says it. A word the station adds later is
 *  shown as it is rather than guessed at. */
export function fromWords(from: HostOrigin | string | null | undefined): string | null {
  if (!from) return null;
  if (from === 'room') return 'in the room';
  if (from === 'push') return 'own studio';
  if (from === 'pull') return 'pulled by the station';
  return String(from);
}

/** *Eilidh*, *Eilidh and Gregor*, *Eilidh, Gregor and Kirsty*. */
export function names(list: readonly string[]): string {
  const n = list.map((x) => firstName(x, x)).filter(Boolean);
  if (n.length <= 1) return n[0] ?? '';
  return `${n.slice(0, -1).join(', ')} and ${n.at(-1)}`;
}

function sentence(r: Omit<WindowRow, 'said'>): string {
  if (r.is === 'overrun') return `${r.who} ${r.who.includes(' and ') ? 'are' : 'is'} running over. ${r.what}.${r.note ? ` ${r.note}.` : ''}`;
  const lead = r.is === 'late' ? 'Late' : 'Next';
  const tail = [r.from, r.readiness ? READY_WORD[r.readiness] : null, r.note].filter(Boolean).join(', ');
  return `${lead}: ${r.who}, ${r.what}${tail ? `, ${tail}` : ''}.`;
}

const row = (r: Omit<WindowRow, 'said'>): WindowRow => ({ ...r, said: sentence(r) });

/** The window's rows for the desk, at most two; none while the window is quiet or the station
 *  sends none (a station older than 1.2, the window off, or a show nobody is down for). */
export function windowRows(post: PostView | null | undefined, now: number): WindowRow[] {
  const w = post?.window;
  if (!post || !w || w.state === 'quiet') return [];
  const rows: WindowRow[] = [];
  const mine = post.show?.slug === w.show && post.show.start_ms === w.start_ms;
  const what = `${w.show_name} · ${hm(w.start_ms)}`;

  // The show under way still waits for its host, and the window has moved on to the next show:
  // the late one first. The station says who (1.2.1); an older one leaves it to the post.
  if (w.late !== undefined) {
    const l = w.late;
    if (l)
      rows.push(
        row({
          is: 'late',
          label: 'Late',
          who: l.host,
          what: `${l.show_name} · since ${hm(l.start_ms)}`,
          show: l.show,
          from: fromWords(l.from),
          note: null,
          readiness: l.state === 'ready' ? 'ready' : l.state === 'connecting' ? 'connecting' : 'not_here',
        }),
      );
  }
  const waiting = w.late === undefined && post.show && !mine && post.show.started ? ofPost(post.state) : null;
  if (post.show && waiting && post.source_name) {
    rows.push(
      row({
        is: 'late',
        label: 'Late',
        who: post.source_name,
        what: `${post.show.name} · since ${hm(post.show.start_ms)}`,
        show: post.show.slug,
        from: null,
        note: null,
        readiness: waiting,
      }),
    );
  }

  if (w.state === 'overrun') {
    const over = names(post.holders);
    const next = mine ? ofPost(post.state) : null;
    rows.push(
      row({
        is: 'overrun',
        label: 'Over',
        who: over || 'The show before',
        what: `${w.show_name} was due ${hm(w.start_ms)}`,
        show: w.show,
        from: null,
        note: next ? `${firstName(w.host, w.host)} is ${READY_WORD[next]}` : null,
        readiness: null,
      }),
    );
    return rows;
  }

  // Announced, connecting, ready, or held: who is next. A hold keeps the window open past the
  // post; how far the host has got is then the post's own reading.
  const held = w.state === 'held';
  const told = w.heads_up_ms != null && w.start_ms > now ? `heads-up ${hm(w.heads_up_ms)}` : null;
  rows.push(
    row({
      is: 'next',
      label: 'Next',
      who: w.host,
      what,
      show: w.show,
      from: fromWords(w.from),
      // The show is in the rehearsal studio now (1.3): the window says so. Its people are moved
      // to the live studio by a press there, never by the window.
      note: [held ? `held${post.held_by ? ` by ${post.held_by}` : ''}: a press only` : told, w.rehearsing ? 'rehearsing' : null].filter(Boolean).join(' · ') || null,
      readiness: held ? (mine ? ofPost(post.state) : null) : ofWindow(w),
    }),
  );
  return rows;
}

/** The heads-up's notice for this window, if the station has given it: the newest
 *  `handover.heads_up` since the window opened. */
export function headsUpNotice(v: Pick<View, 'notices' | 'post'> | null | undefined): Notice | null {
  const w = v?.post?.window;
  if (!v || !w) return null;
  return v.notices.filter((n) => n.kind === 'handover.heads_up' && n.at_ms >= w.opens_ms).at(-1) ?? null;
}

/** The seat notice: the tile of the host the window is about, while they wait for their post.
 *  A host in their own studio is found by their show; a presenter in the room by the heads-up's
 *  subject, or by the post's own source. Null once they are on air, after the post, or when
 *  they are not at the desk. */
export function nextSeat(v: Pick<View, 'notices' | 'post' | 'sources'> | null | undefined, now: number): { identity: string; words: string } | null {
  const post = v?.post;
  const w = post?.window;
  if (!v || !post || !w || w.state === 'quiet' || w.state === 'overrun' || w.start_ms <= now) return null;
  const subject = headsUpNotice(v)?.subject ?? null;
  const mine = post.show?.slug === w.show;
  const at = (s: Source) =>
    s.identity === subject || (s.role === 'source' ? s.source?.show === w.show : mine && !!post.source && s.identity === post.source);
  const seat = v.sources.find((s) => !s.on_air && at(s));
  if (!seat) return null;
  return {
    identity: seat.identity,
    words: `On next · ${w.show_name} at ${hm(w.start_ms)}${w.heads_up_ms != null ? ` · told ${hm(w.heads_up_ms)}` : ''}`,
  };
}

// ---- the rules: the window's length ----

/** The lengths the rules offer (the operator, 2026-11-20): off, or 5 to 30 minutes. */
export const WINDOWS: readonly { s: number; label: string }[] = [
  { s: 0, label: 'Off' },
  { s: 300, label: '5 minutes before' },
  { s: 600, label: '10 minutes before' },
  { s: 900, label: '15 minutes before' },
  { s: 1800, label: '30 minutes before' },
];
export const WINDOW_MAX_S = 1800;

/** The choices for a station's stored length: the five, and its own where it is another (set
 *  through the API), shown as it is rather than moved to the nearest. */
export function windowChoices(stored: number | null | undefined): { s: number; label: string }[] {
  if (stored == null || WINDOWS.some((w) => w.s === stored)) return [...WINDOWS];
  const min = Math.floor(stored / 60);
  const sec = stored % 60;
  const label = `${min ? `${min} min` : ''}${min && sec ? ' ' : ''}${sec ? `${sec} s` : ''} before (set through the API)`;
  return [...WINDOWS, { s: stored, label }].sort((a, b) => a.s - b.s);
}
