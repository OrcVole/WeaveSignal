// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The post, the grace, the late count and a dropped link at the desk (ADR-0280;
// hosts-in-their-own-studios.md sections 3.4 to 3.6, 4.1, 4.2 and 8), as pure rules worked out
// from the studio's view alone: what the clock's post line says, the rundown's row for the
// show whose post it is, the tile of a source whose place is kept, which toast the desk shows
// first, the next show's source in the guest's seat at Solo, and where End show sends the air. The station decides everything (the post's rule runs
// there every second, `post` in the view); this only puts it into the desk's words.
//
// What the view does not carry is not guessed here: how long a source must be steady (5 s, or
// 30 s after three drops in ten minutes) is the station's rule and an admin's setting, so a
// returning source's tile says it is back on air once steady, without a count. The station
// names who holds a handover by their username, and that is shown as given.

import type { LinkLost, Me, PostView, Source, View } from './api.ts';
import { dur, firstName, hm, hms } from './format.ts';
import { CHIP, type Chip, type StationMode } from '../../../../packages/ui/states.ts';

/** How far ahead a post is said on the clock and the rundown. Their times are a clock's,
 *  without a date, and the station names the next post up to a week ahead. */
export const POST_AHEAD_MS = 12 * 3600_000;
/** A count turns amber in its last 30 s and red in its last 10 s (14a; the monitor page's rule,
 *  apps/web/src/lib/monitor/words.ts). */
export const AMBER_S = 30;
export const RED_S = 10;
/** How long the desk says a source is back on air. */
export const BACK_NOTE_MS = 10_000;

const first = (name: string) => firstName(name, name);

/** How pressing a count is. The colour is never the only sign: the count's mark (the clock
 *  glyph, one shape at both levels; never FAULT's triangle) stands before the figure, the figure
 *  is heavier, and the word is for a screen reader. */
export interface Hurry {
  level: 'amber' | 'red';
  word: string;
}

/** A count's last seconds, judged by the figure the desk shows (`dur` drops the fraction, so
 *  `0:30` is the first amber and `0:10` the first red); null while it has more, and once it has
 *  run out. */
export function hurry(left_ms: number): Hurry | null {
  if (left_ms <= 0) return null;
  const s = Math.floor(left_ms / 1000);
  if (s > AMBER_S) return null;
  return s <= RED_S ? { level: 'red', word: 'Last 10 seconds' } : { level: 'amber', word: 'Last 30 seconds' };
}

/** `Finlay’s`, `Angus’`. */
export const whose = (name: string) => `${name}’${/s$/i.test(name) ? '' : 's'}`;

/** What covers a dropped link: automation, or on a station without the playout layer the
 *  listener process's own fallback (design section 12). */
export const coverWord = (mode: StationMode | null | undefined) => (mode === null || mode === undefined ? 'the fallback' : 'automation');

/** An admin, a producer or the station key holds a handover and lets it run (the control
 *  plane's `may_hold`); an API token never, and it has no desk. */
export function mayHold(me: Me | null | undefined): boolean {
  return !!me && (me.kind === 'station' || me.role === 'admin' || me.role === 'producer');
}

/** Who holds the handover, as the station names them: its log's label, a username. */
export function heldBy(by: string, me: Me | null | undefined): string {
  if (by === 'station key') return 'the station key';
  return me?.kind === 'staff' && me.account?.username === by ? 'you' : by;
}

// ---- the clock's post line ----

export type PostLine =
  /** Before the post: *Steeped · the post 14:00:00*, counted down. */
  | { kind: 'post'; name: string; at_ms: number }
  /** The schedule's own count: the grace with a DJ on air, or the late count with automation
   *  holding the air. `key` is the source's show key for the late count's two buttons. */
  | { kind: 'count'; why: 'grace' | 'late'; name: string; at_ms: number; from_ms: number; key: string | null }
  /** The grace is held: the handover is by a press only. */
  | { kind: 'held'; by: string }
  /** Nothing is counted: the DJ on air keeps the air, or the next host waits for a press. */
  | { kind: 'words'; text: string };

/** The post line for the show whose post it is, or null when the schedule has nothing to say
 *  (the rundown's own hard post, if any, keeps the line then). */
export function postLine(post: PostView | null | undefined, now: number): PostLine | null {
  const show = post?.show;
  if (!post || !show) return null;
  // Only a show with a host to bring on has a post of the schedule's to say (`state` is null for
  // a show that comes from the room: the desk is as it was for those).
  if (!show.started) return post.state != null && show.start_ms > now && show.start_ms - now <= POST_AHEAD_MS ? { kind: 'post', name: show.name, at_ms: show.start_ms } : null;
  if (post.held_by) return { kind: 'held', by: post.held_by };
  const since = Math.max(show.start_ms, post.standing_since_ms ?? show.start_ms);
  if (post.grace_ends_ms != null) return { kind: 'count', why: 'grace', name: show.name, at_ms: post.grace_ends_ms, from_ms: Math.min(since, post.grace_ends_ms), key: null };
  if (post.late_ends_ms != null) return { kind: 'count', why: 'late', name: show.name, at_ms: post.late_ends_ms, from_ms: Math.min(since, post.late_ends_ms), key: post.source ?? null };
  const who = post.source_name ? first(post.source_name) : '';
  // A source back from a lost link is the station's to put back on air by itself: its tile says so.
  if (!who || returning(post)) return null;
  // Standing by, and nothing counts: the grace is off, or the count was stopped. A press only.
  if (post.state === 'standing_by') return { kind: 'words', text: `${who} is standing by · a press only` };
  // The DJ on air keeps the air until the next host is standing by (design section 3.4).
  if (!post.holders.length) return null;
  if (post.state === 'not_connected' || post.state === 'link_lost') return { kind: 'words', text: `${who} has not connected. You keep the air.` };
  if (post.state === 'connecting') return { kind: 'words', text: `${who} is connecting. You keep the air.` };
  return null;
}

/** The count's words: *Kettle Drive takes the air* and *by the schedule 16:02:00*. */
export function countWords(line: Extract<PostLine, { kind: 'count' }>): { name: string; by: string } {
  return { name: `${line.name} takes the air`, by: `by the schedule ${hms(line.at_ms)}` };
}

/** How much of the count has run, for the bar. */
export function countPct(line: { at_ms: number; from_ms: number }, now: number): number {
  const whole = line.at_ms - line.from_ms;
  return whole > 0 ? Math.max(0, Math.min(100, ((now - line.from_ms) / whole) * 100)) : 100;
}

// ---- the source the post is about, at the desk ----

/** The post's source is the one whose link was lost: reconnected or not, its place is kept and
 *  the station puts it back by itself, so the desk offers no press for it and counts nothing. */
const returning = (post: PostView) => !!post.link_lost && post.link_lost.key_id === post.source;

/** The sources whose arrival is news: not one that has only come back to a place kept for it. */
export function arrivals<T extends Pick<Source, 'role' | 'source'>>(v: { sources: readonly T[]; post?: PostView | null } | null | undefined): T[] {
  const lost = v?.post?.link_lost?.key_id;
  return (v?.sources ?? []).filter((s) => !lost || s.source?.key !== lost);
}

/** The source at the desk that a show key admitted. */
export function sourceOfKey<T extends Pick<Source, 'role' | 'source'>>(sources: readonly T[], key: string | null | undefined): T | null {
  return key ? (sources.find((s) => s.role === 'source' && s.source?.key === key) ?? null) : null;
}

/** The source one press of the handover's key puts on air (design sections 3.5 and 6): its
 *  show's post has come, it is standing by, and nobody presents, so there is nobody to take
 *  off. `counting` is the late count running: the station does it by itself at `at_ms`. */
export function goNow(v: View | null | undefined): { source: Source; key: string; counting: boolean; at_ms: number | null } | null {
  const post = v?.post;
  if (!v || !post?.show?.started || post.state !== 'standing_by' || post.holders.length || returning(post)) return null;
  const source = sourceOfKey(v.sources, post.source);
  if (!source || source.on_air || !source.source) return null;
  return { source, key: source.source.key, counting: post.late_ends_ms != null, at_ms: post.late_ends_ms ?? null };
}

/** The late count the station is running for a source at the desk (design section 3.5): it
 *  goes on air by itself at `at_ms` unless told Not yet. The station counts whenever automation
 *  holds the air, even with a presenter open to the room, so the count and Not yet do not wait
 *  on the press being offered (`goNow`). */
export function lateCount(v: View | null | undefined): { source: Source; key: string; at_ms: number } | null {
  const post = v?.post;
  if (!v || !post || post.late_ends_ms == null || returning(post)) return null;
  const source = sourceOfKey(v.sources, post.source);
  if (!source || source.on_air || !source.source) return null;
  return { source, key: source.source.key, at_ms: post.late_ends_ms };
}

/** The next show's source standing by while someone presents (design sections 3.3 and 6): the
 *  handover's key picks it before any DJ waiting, and its first press brings it on air beside
 *  whoever holds the air. The grace's toast offers the same press. */
export function handNext(v: View | null | undefined): Source | null {
  const post = v?.post;
  if (!v || !post || post.state !== 'standing_by' || !post.holders.length || returning(post)) return null;
  const source = sourceOfKey(v.sources, post.source);
  return source && !source.on_air ? source : null;
}

// ---- the rundown's row ----

export interface PostRow {
  at_ms: number;
  title: string;
  /** *Finlay Grant · own studio*. */
  detail: string;
  /** How its source stands; null when there is nothing to say yet. */
  chip: Chip | null;
  /** Its post has come: the row is the one under way. */
  now: boolean;
  /** The source's identity at the desk, when it is there. */
  identity: string | null;
}

/** The row for the show whose post it is (design section 4.2): where it comes from and how its
 *  source stands. The view does not say whether a slot's door is open, so NOT CONNECTED is
 *  said only once the post has come. Null when the show has no host to bring on. */
export function postRow(v: View | null | undefined, now: number): PostRow | null {
  const post = v?.post;
  const show = post?.show;
  if (!v || !post || !show || !post.source || !post.state) return null;
  if (!show.started && show.start_ms - now > POST_AHEAD_MS) return null;
  const own = sourceOfKey(v.sources, post.source);
  const inRoom = !own && v.sources.some((s) => s.identity === post.source && s.role !== 'source');
  const chip: Chip | null =
    post.state === 'on_air'
      ? CHIP.onAir
      : post.state === 'link_lost' || returning(post)
        ? CHIP.linkLost
        : post.state === 'standing_by'
          ? CHIP.standby
          : show.started && post.state === 'not_connected'
            ? CHIP.notConnected
            : null;
  return {
    at_ms: show.start_ms,
    title: show.name,
    detail: [post.source_name ?? '', inRoom ? 'in the room' : 'own studio'].filter(Boolean).join(' · '),
    chip,
    now: show.started,
    identity: own?.identity ?? (inRoom ? post.source : null),
  };
}

// ---- a lost link: the place kept on the stage ----

/** Whether a lost link is covered. With the playout layer the station says so (`covered`, when
 *  it has handed the air to automation). Without it there is no automation and the station
 *  never does: the listener process's own fallback covers once the silence is over
 *  (`covers_ms`) with nobody else on air, and that is worked out here from the view's clock. */
export function isCovered(v: View | null | undefined): boolean {
  const lost = v?.post?.link_lost;
  if (!v || !lost) return false;
  if (lost.covered) return true;
  return (v.mode === null || v.mode === undefined) && v.now_ms >= lost.covers_ms && !v.post?.holders.length;
}

/** A source's place while its link is lost: what the kept tile says. */
export interface Kept {
  key: string;
  identity: string;
  name: string;
  slot: number;
  lost_ms: number;
  until_ms: number;
  covered: boolean;
  /** Its software has reconnected: the source is at the desk again, off air until steady. */
  back: boolean;
}

/** A source takes strip 5, then 1, 2, 3 (design section 4.1): where a place this window never
 *  saw filled is drawn. */
export function freeSlot(sources: readonly { slot: number }[]): number {
  return [5, 1, 2, 3].find((n) => !sources.some((s) => s.slot === n)) ?? 5;
}

/** The kept place, from the view's lost link. `slots` is where this window last saw each key's
 *  source (`studio.sourceSlots`): the view does not carry the strip of a source that has left. */
export function keptPlace(v: View | null | undefined, slots: Readonly<Record<string, number>> = {}): Kept | null {
  const lost = v?.post?.link_lost;
  if (!v || !lost) return null;
  const at = sourceOfKey(v.sources, lost.key_id);
  // On air again: the station's log follows within a second; the place is no longer lost.
  if (at?.on_air) return null;
  const remembered = slots[lost.key_id];
  const slot = at?.slot ?? (remembered && !v.sources.some((s) => s.slot === remembered) ? remembered : freeSlot(v.sources));
  return { key: lost.key_id, identity: at?.identity ?? `source-${lost.key_id}`, name: lost.name, slot, lost_ms: lost.lost_ms, until_ms: lost.place_kept_until_ms, covered: isCovered(v), back: !!at };
}

/** The tile's stand-in for a source that has left the desk: its name and strip, and nothing
 *  that sounds. */
export function keptSource(k: Kept): Source {
  return { identity: k.identity, name: k.name, role: 'source', on_air: false, mute: false, cue: false, talk: false, talk_to: null, present: false, away_ms: null, linked: false, lufs: null, lufs_peak: null, slot: k.slot, source: null, caller: null };
}

/** The stage with the kept place on it, in slot order: a lost source's tile stays where it was
 *  (design section 3.6), and one that has reconnected does not drop to Not on air for the few
 *  seconds before it is back. */
export function withKept(placed: { stage: Source[]; waiting: Source[] }, k: Kept | null): { stage: Source[]; waiting: Source[] } {
  if (!k) return placed;
  const here = [...placed.stage, ...placed.waiting].find((s) => s.identity === k.identity) ?? keptSource(k);
  const stage = placed.stage.filter((s) => s.identity !== k.identity);
  const after = stage.findIndex((s) => s.slot > here.slot);
  stage.splice(after < 0 ? stage.length : after, 0, here);
  return { stage, waiting: placed.waiting.filter((s) => s.identity !== k.identity) };
}

/** The tile's link line (design section 8): *0:07 · place kept until 14:46*; once its software
 *  is back, that it returns by itself, or with someone else on air that it stands by (the
 *  station puts a returning source back only when nobody else presents). */
export function keptLine(k: Kept, now: number, others = false): string {
  const left = keptLeft(k, now);
  return `${keptHeld(k, now, others)}${left ? ` · ${left} left` : ''}`;
}

/** The line before its last seconds' count: how long the link has been lost and until when the
 *  place is kept, or what a source that is back waits for. */
export function keptHeld(k: Kept, now: number, others = false): string {
  if (k.back) return others ? 'Reconnected · standing by' : 'Back on air once steady';
  return `${dur(now - k.lost_ms)} · place kept until ${hm(k.until_ms)}`;
}

/** What is left of a kept place in its last 30 s, *0:05*: the figure the tile draws with the
 *  count's mark before it. Null before then, and for a source that is back. */
export function keptLeft(k: Kept, now: number): string | null {
  if (k.back) return null;
  const left = k.until_ms - now;
  return hurry(left) ? dur(left) : null;
}

/** The one line under the kept tile, where there is something the lines above do not say: what
 *  listeners hear while it is covered, or that a returning source waits for a press. One line:
 *  a kept tile shares the stage with whatever else is on it. */
export function keptNote(k: Kept, mode: StationMode | null | undefined, others: boolean): string | null {
  if (k.back) return others ? 'Not put back by itself while someone else has the air.' : null;
  return k.covered ? `Listeners hear ${coverWord(mode)} until the stream is steady.` : null;
}

/** The kept tile's own On key, lit while the place is kept: pressing it is Off air. */
export const OFF_AIR_WHY = 'Off air: lets their place go now. If they reconnect, they stand by.';

/** After Off air on a kept place (design section 3.6): the place is let go, and where the air
 *  is. `covered` is the station's own word that automation took the air with the press;
 *  otherwise the air stays where the view had it: with whoever presents, with automation, or on
 *  a station without the playout layer with the listener process's fallback. */
export function letGoWords(name: string, covered: boolean, v: Pick<View, 'mode' | 'post'> | null | undefined): { title: string; message: string } {
  const who = first(name);
  const holders = (v?.post?.holders ?? []).map(first);
  const air = covered
    ? 'Automation has taken the air.'
    : holders.length
      ? `${holders.join(' and ')} ${holders.length === 1 ? 'keeps' : 'keep'} the air.`
      : v?.mode === 'auto'
        ? 'Automation holds the air.'
        : v?.mode === null || v?.mode === undefined
          ? 'Listeners hear the fallback.'
          : 'Nobody is on air.';
  return { title: `${whose(who)} place is let go`, message: `${air} If ${who} reconnects, they stand by.` };
}

/** The stage's subline while automation covers a lost link (the design's drawn desk). */
export function keptSub(k: Kept | null): string | null {
  return k && k.covered ? `${whose(first(k.name))} place is kept until ${hm(k.until_ms)}` : null;
}

// ---- the toasts ----

export interface HostToast {
  title: string;
  message: string;
}

/** A lost link once it is covered (design sections 3.6 and 8): with someone else on air only
 *  the tile changes, so there is no toast then. Dismissed by `lost_ms`, so a second drop is
 *  said again. */
export function lostToast(v: View | null | undefined, dismissed: readonly string[]): (HostToast & { id: string; key: string }) | null {
  const lost = v?.post?.link_lost;
  if (!v || !lost || !isCovered(v)) return null;
  const id = lostId(lost);
  if (dismissed.includes(id) || sourceOfKey(v.sources, lost.key_id)) return null;
  const cover = coverWord(v.mode);
  return {
    id,
    key: lost.key_id,
    title: `${whose(first(lost.name))} link is lost`,
    message: `${cover[0].toUpperCase()}${cover.slice(1)} covers. Their place is kept until ${hm(lost.place_kept_until_ms)}.`,
  };
}

export const lostId = (lost: LinkLost) => `${lost.key_id}@${lost.lost_ms}`;

/** A view as this window knows it after Off air: the station's `post` follows a decision by up
 *  to a second, so a view that still carries the place this window was told is let go is read
 *  without it. Otherwise the kept tile and its toast would come back for a moment, and a second
 *  press would be sent to be refused. */
export function withoutLetGo<V extends Pick<View, 'post'>>(v: V, letGo: readonly string[]): V {
  const post = v.post;
  const lost = post?.link_lost;
  if (!post || !lost || !letGo.includes(lostId(lost))) return v;
  return { ...v, post: { ...post, link_lost: null, state: post.source === lost.key_id && post.state === 'link_lost' ? 'not_connected' : post.state } };
}

/** The late count (design sections 3.5 and 8): *Finlay goes on air in 0:20*. */
export function lateToast(v: View | null | undefined, now: number): (HostToast & { key: string; left_ms: number; lead: string; tick: string; said: string }) | null {
  const go = lateCount(v);
  if (!go) return null;
  const left = Math.max(0, go.at_ms - now);
  const lead = `${first(go.source.name)} goes on air`;
  return {
    key: go.key,
    left_ms: left,
    title: `${lead} in ${dur(left)}`,
    // The toast is a live region and the count moves every second: a screen reader is told
    // the time it ends, once, and the moving figure is for the eye.
    lead,
    tick: `in ${dur(left)}`,
    said: `at ${hms(go.at_ms)}`,
    message: 'They connected late. Automation holds their air.',
  };
}

/** The grace's offer (design sections 3.4 and 8): *Heather goes on air in 0:42*, by the
 *  schedule, or by a press now. `press` is whether this window offers the handover's key (Full,
 *  and not preparing); without it the other way is End show. Dismissed by the handover it is
 *  for (Not now): the grace runs on, and the clock goes on counting it. */
export function graceToast(v: View | null | undefined, now: number, dismissed: readonly string[] = [], press = false): (HostToast & { id: string; left_ms: number; lead: string; tick: string; said: string }) | null {
  const post = v?.post;
  if (!post?.show?.started || post.grace_ends_ms == null || post.held_by || !post.source || !post.source_name || post.state !== 'standing_by') return null;
  const id = graceId(post);
  if (dismissed.includes(id)) return null;
  const left = Math.max(0, post.grace_ends_ms - now);
  const lead = `${first(post.source_name)} goes on air`;
  return {
    id,
    left_ms: left,
    title: `${lead} in ${dur(left)}`,
    lead,
    tick: `in ${dur(left)}`,
    said: `at ${hms(post.grace_ends_ms)}`,
    message: `By the schedule, at ${hms(post.grace_ends_ms)}. ${press ? 'Or bring them on now.' : 'Or end the show, and the air passes now.'}`,
  };
}

/** One offer a handover: the same grace, held and let run again, is not offered twice. */
export const graceId = (post: PostView) => `${post.source}@${post.show?.start_ms ?? 0}`;

/** A source is back on air after a lost link: its key was lost in the last view and is on air
 *  in this one. The station posts no notice for it, so the window says it from what it saw. */
export function backOnAir(before: View | null | undefined, after: View): HostToast | null {
  const lost = before?.post?.link_lost;
  if (!lost) return null;
  const now = sourceOfKey(after.sources, lost.key_id);
  if (!now?.on_air || sourceOfKey(before?.sources ?? [], lost.key_id)?.on_air) return null;
  const who = first(now.name || lost.name);
  return {
    title: `${who} is back on air`,
    message: isCovered(before) ? `Their link held steady, and ${coverWord(after.mode)} has handed the air back.` : 'Their link held steady.',
  };
}

/** Which of the desk's toasts with an answer is showing: the most urgent first (design section
 *  4.2: a lost link, a grace running, a late host's count, a source standing by, in that
 *  order; a caller who is ready and a degraded link keep their places ahead of the grace). One
 *  rule for the toast and for Esc, so Esc answers the toast that is on screen and never one
 *  behind it. */
export type DeskToastKind = 'playout' | 'lost' | 'caller' | 'degraded' | 'grace' | 'late' | 'arrived';

export function deskToastKind(f: { playoutDown: boolean; lost: boolean; caller: boolean; degraded: boolean; grace?: boolean; late: boolean; arrived: boolean }): DeskToastKind | null {
  if (f.playoutDown) return 'playout';
  if (f.lost) return 'lost';
  if (f.caller) return 'caller';
  if (f.degraded) return 'degraded';
  if (f.grace) return 'grace';
  if (f.late) return 'late';
  if (f.arrived) return 'arrived';
  return null;
}

// ---- `desk.next`: the next show's source in the guest's seat, at Solo ----

/** What the card says of when its source goes on air. `count` is the late count, drawn as a
 *  moving figure with Not yet; `words` is the sentence otherwise. */
export interface NextCard {
  source: Source;
  key: string;
  /** *Steeped · Finlay Grant*. */
  title: string;
  /** *Own studio · SRT · steady 4:51*. */
  link: string;
  words: string;
  count: { at_ms: number; key: string } | null;
}

/** The source the card is for (design section 4.2): the post's own, at the desk and off air.
 *  Not one whose place is kept: that stays a tile on the stage until the station puts it back
 *  or lets it go. */
export function nextSource(v: View | null | undefined): Source | null {
  const post = v?.post;
  if (!v || !post?.show || returning(post)) return null;
  const source = sourceOfKey(v.sources, post.source);
  return source && !source.on_air && source.source ? source : null;
}

/** The card (design sections 4.2 and 8): *Next · Steeped · Finlay Grant · on air by the
 *  schedule at 16:02:00, or when you end the show*. The sentence follows the clock's post line
 *  case by case, and says only what the view carries: before the post the grace's length is
 *  the station's setting, so no time is worked out from a guess. *When you end the show* is
 *  said while someone presents and the station would pass the air (`endsTo`). */
export function nextCard(v: View | null | undefined, link: (s: NonNullable<Source['source']>) => string): NextCard | null {
  const source = nextSource(v);
  const post = v?.post;
  const show = post?.show;
  if (!v || !source?.source || !post || !show) return null;
  const who = first(source.name);
  const ends = post.holders.length > 0 && !!endsTo(v);
  const or = ends ? ', or when you end the show.' : '.';
  const late = lateCount(v);
  let words: string;
  if (late) words = 'by the schedule';
  else if (post.state !== 'standing_by') words = 'Standing by once their link is steady.';
  else if (!show.started) words = post.on_air_at_ms != null && !post.holders.length ? `On air by the schedule at ${hms(post.on_air_at_ms)}.` : `The post is at ${hms(show.start_ms)}${ends ? '. Or on air when you end the show.' : '.'}`;
  else if (post.held_by) words = `Handover held by ${post.held_by === 'station key' ? 'the station key' : post.held_by}${ends ? ': on air when you end the show.' : '.'}`;
  else if (post.grace_ends_ms != null) words = `On air by the schedule at ${hms(post.grace_ends_ms)}${or}`;
  else if (post.on_air_at_ms != null) words = `On air by the schedule at ${hms(post.on_air_at_ms)}.`;
  else if (post.holders.length) words = ends ? 'On air when you end the show.' : 'The schedule is not counting them on.';
  else words = `Nothing is counted: ${who} goes on air by their own press.`;
  return { source, key: source.source.key, title: `${show.name} · ${source.name}`, link: `Own studio · ${link(source.source)}`, words, count: late ? { at_ms: late.at_ms, key: late.key } : null };
}

/** The stage at Solo without the source the card stands for: it has the guest's seat, not a tile. */
export function withoutNext(placed: { stage: Source[]; waiting: Source[] }, next: Source | null): { stage: Source[]; waiting: Source[] } {
  return next ? { stage: placed.stage.filter((s) => s.identity !== next.identity), waiting: placed.waiting } : placed;
}

// ---- the foot ----

/** The foot's item with a lost link in it: *1 link lost*, with whatever else is connected. */
export function ownStudiosLost(words: string, k: Kept | null): string {
  return k && !k.back ? [words, '1 link lost'].filter(Boolean).join(' · ') : words;
}

// ---- End show ----

/** Where End show sends the air (design sections 3.3, 3.8 and 8): to the next host when the
 *  schedule still has them standing by to bring on, else to automation. The station passes the
 *  air only to the source its rule would bring on by itself (`standing_since_ms`): one told
 *  Not yet, or taken off air since the post, stays off. */
export function endsTo(v: View | null | undefined): { name: string; show: string } | null {
  const post = v?.post;
  if (!post?.show || post.state !== 'standing_by' || post.standing_since_ms == null || !post.source_name) return null;
  return { name: first(post.source_name), show: post.show.name };
}

/** The End show button's own words on where the air goes, before the press. */
export function endsToWords(v: View | null | undefined): string | null {
  const to = endsTo(v);
  if (to) return `${to.name} is standing by: ${to.show} starts as you end.`;
  if (!v || v.mode === null || v.mode === undefined) return null;
  return v.mode === 'auto' ? 'Automation already holds the air.' : 'Automation takes the air.';
}
