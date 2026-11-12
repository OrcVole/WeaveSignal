// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The DJ's show setup (ADR-0218, ADR-0219), the rules without a page: whose show a show is,
// whether a newly signed-in presenter is shown the wizard, the short list of answers and how
// they become a show, and what each answer must look like. The answers are kept in the
// station as the show's own fields, so the consent words name the DJ while the show is on
// air (ADR-0214) and new episodes take the usual slot.

import type { Me } from './api.ts';
import type { Show, ShowInput, ShowView, Slot } from './schedule.ts';
import { slugOf } from '../schedule/model.ts';

/** The wizard's answers: ADR-0219's list and nothing more. */
export interface Answers {
  presenter: string;
  contact: string;
  show: string;
  /** One link per line as typed; split and trimmed when saved. */
  links: string;
  weekday: number;
  /** `HH:MM` in station time, or empty for no usual slot. */
  from: string;
  /** `HH:MM`; empty leaves the length open. */
  until: string;
  commercial_music: boolean;
}

export const PRESENTER_MAX = 120;
export const CONTACT_MAX = 200;
export const SHOW_NAME_MAX = 160;
export const LINKS_MAX = 10;
export const LINK_MAX = 400;

/** The show a member of staff presents: the one tied to their account (ADR-0218). */
export function myShow(me: Me | null | undefined, shows: readonly Show[]): Show | null {
  const id = me?.kind === 'staff' ? me.account?.id : undefined;
  if (!id) return null;
  return shows.find((s) => s.presenter_account === id) ?? null;
}

/** Whether to show the wizard by itself: a presenter signed in as themselves, with no show
 *  tied to their account, who has not skipped it on this machine. Admins, producers and
 *  screeners find it in Settings; the station key has no account to tie a show to. */
export function needsSetup(me: Me | null | undefined, shows: readonly Show[], skipped: boolean): boolean {
  if (!me || me.kind !== 'staff' || me.role !== 'presenter' || !me.account?.id) return false;
  return !skipped && myShow(me, shows) === null;
}

/** The key under which this machine remembers that an account skipped the wizard. */
export const skipKey = (accountId: string): string => `weavesignal.setup.skipped.${accountId}`;

const pad = (n: number) => String(n).padStart(2, '0');
export const hhmm = (minutes: number): string => `${pad(Math.floor(minutes / 60) % 24)}:${pad(minutes % 60)}`;

/** The answers a show already holds, for the wizard run again (ADR-0218). */
export function fromShow(show: Show | null, me: Me | null | undefined): Answers {
  const slot = show?.slot ?? null;
  return {
    presenter: show?.presenter || (show ? '' : (me?.account?.name ?? '')),
    contact: show?.contact ?? '',
    show: show?.name ?? '',
    links: (show?.links ?? []).join('\n'),
    weekday: slot?.weekday ?? 6,
    from: slot ? hhmm(slot.start) : '',
    until: slot?.minutes ? hhmm(slot.start + slot.minutes) : '',
    commercial_music: show?.commercial_music ?? false,
  };
}

/** `HH:MM` as minutes after midnight, or null when it is not a time. */
export function minutesOf(t: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(t.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const mm = Number(m[2]);
  if (h > 23 || mm > 59) return null;
  return h * 60 + mm;
}

/** An end before the start: the slot runs past midnight, which the wizard asks to confirm
 *  (ADR-0244), since 20:00 to 19:00 is more often a typo than a 23-hour show. */
export function overnight(a: Pick<Answers, 'from' | 'until'>): boolean {
  const start = minutesOf(a.from);
  const end = minutesOf(a.until);
  return start !== null && end !== null && end < start;
}

/** The usual slot from the day and times: `until` before `from` runs past midnight. */
export function slotFrom(a: Pick<Answers, 'weekday' | 'from' | 'until'>): Slot | null {
  const start = minutesOf(a.from);
  if (start === null) return null;
  const end = minutesOf(a.until);
  let minutes: number | null = null;
  if (end !== null) {
    minutes = end > start ? end - start : end + 1440 - start;
    if (minutes === 0) minutes = 1440;
  }
  return { weekday: a.weekday, start, minutes };
}

export function splitLinks(s: string): string[] {
  return s
    .split(/[\n,;]/)
    .map((x) => x.trim())
    .filter(Boolean);
}

/** Problems with the first step's answers, by field. */
export function aboutProblems(a: Answers): { presenter?: string; contact?: string } {
  const out: { presenter?: string; contact?: string } = {};
  const p = a.presenter.trim();
  if (!p) out.presenter = 'Add the name you go by on air.';
  else if (p.length > PRESENTER_MAX) out.presenter = `At most ${PRESENTER_MAX} characters.`;
  else if (/[\p{Cc}]/u.test(p)) out.presenter = 'One line, please.';
  const c = a.contact.trim();
  if (c.length > CONTACT_MAX) out.contact = `At most ${CONTACT_MAX} characters.`;
  else if (/[\p{Cc}]/u.test(c)) out.contact = 'One line, please.';
  return out;
}

/** Problems with the second step's answers, by field. */
export function showProblems(a: Answers): { show?: string; links?: string; time?: string } {
  const out: { show?: string; links?: string; time?: string } = {};
  const n = a.show.trim();
  if (!n) out.show = "Add your show's name.";
  else if (n.length > SHOW_NAME_MAX) out.show = `At most ${SHOW_NAME_MAX} characters.`;
  else if (!slugOf(n)) out.show = 'The name needs at least one letter or digit.';
  const links = splitLinks(a.links);
  if (links.length > LINKS_MAX) out.links = `At most ${LINKS_MAX} links.`;
  else {
    const bad = links.find((l) => l.length > LINK_MAX || /\s/.test(l));
    if (bad) out.links = `“${bad.slice(0, 40)}” is not a link: one address per line, with no spaces.`;
  }
  const from = a.from.trim();
  const until = a.until.trim();
  if (from && minutesOf(from) === null) out.time = 'The start is a time, such as 20:00.';
  else if (until && minutesOf(until) === null) out.time = 'The end is a time, such as 22:00.';
  else if (!from && until) out.time = 'Give the start as well as the end.';
  return out;
}

/** A path for a new show that no other show holds. */
export function freeSlug(name: string, taken: readonly string[]): string {
  const base = slugOf(name) || 'show';
  if (!taken.includes(base)) return base;
  for (let n = 2; n < 100; n++) {
    const s = `${base.slice(0, 57)}-${n}`;
    if (!taken.includes(s)) return s;
  }
  return `${base.slice(0, 40)}-${Date.now().toString(36)}`;
}

/** The PUT body: the answers over what the show already has, tied to this account. An
 *  existing show keeps its description, colour, layout, hosts, tags and explicit flag. */
export function toShowInput(a: Answers, me: Me, existing: ShowView | null, colour: number): ShowInput {
  const s = existing?.show;
  return {
    name: a.show.trim(),
    description: s?.description ?? '',
    colour_slot: s?.colour_slot ?? colour,
    layout: s?.layout ?? 'poster',
    slot: slotFrom(a),
    hosts: existing?.resolved.hosts ?? [],
    tags: s?.tags ?? [],
    explicit: s?.explicit ?? false,
    presenter: a.presenter.trim(),
    contact: a.contact.trim(),
    links: splitLinks(a.links),
    commercial_music: a.commercial_music,
    presenter_account: me.account?.id ?? null,
  };
}

/** The colour for a new show: the next bank slot round the twelve (ADR-0066). */
export const nextColour = (count: number): number => (count % 12) + 1;

export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'] as const;

/** The usual slot in words, for the summary. */
export function slotLine(a: Pick<Answers, 'weekday' | 'from' | 'until'>): string {
  const s = slotFrom(a);
  if (!s) return 'No usual time yet';
  const day = WEEKDAYS[s.weekday - 1] ?? 'Saturday';
  return s.minutes ? `${day}s, ${hhmm(s.start)} to ${hhmm(s.start + s.minutes)}` : `${day}s from ${hhmm(s.start)}`;
}

/** The reminder behind the music question (ADR-0219). */
export const MUSIC_REMINDER =
  'A UK webcast of commercial music needs PPL and PRS for Music licences. This is a reminder for the station; it blocks nothing.';
