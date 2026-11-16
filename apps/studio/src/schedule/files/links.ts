// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Calendar files, the links (Subscribe…, ADR-0281): where the schedule's and a show's `.ics`
// are, as a listener's calendar reaches them.
//
// A link given out is made from the station's public address (`publishing.public_url`, sent
// with the week), never from the address this window talks to: on Cloudron the studio's own
// domain is behind Cloudron's login, and a calendar app sent there would get a login page
// (invitation links had this fault once). With no public address there is no link to give.

/** The schedule's file, and a show's: `schedule.ics`, `the-late-kettle.ics`. */
export const fileName = (show?: string | null): string => `${show || 'schedule'}.ics`;

/** The route, the same on the public address and on the studio's own. */
export const calendarPath = (show?: string | null): string => (show ? `/api/v1/public/shows/${encodeURIComponent(show)}.ics` : '/api/v1/public/schedule.ics');

export interface CalendarLinks {
  /** `webcal://kettle.example/…`: what a calendar app subscribes to. */
  webcal: string;
  /** `https://kettle.example/…`: the same file as a web address. */
  web: string;
}

/** The station's public address as links are made from it: `https://kettle.example`, without
 *  a trailing slash; null when none is set, or when what was sent is not a web address. */
export function publicAddress(raw: string | null | undefined): string | null {
  const p = (raw ?? '').trim().replace(/\/+$/, '');
  return /^https?:\/\/[^\s/"<>]+/i.test(p) ? p : null;
}

/** The links to give out, or null when the station has no public address. */
export function calendarLinks(publicUrl: string | null | undefined, show?: string | null): CalendarLinks | null {
  const p = publicAddress(publicUrl);
  if (!p) return null;
  const web = `${p}${calendarPath(show)}`;
  // `webcal` names no transport of its own: a calendar app asks over https and falls back.
  return { web, webcal: web.replace(/^https?:/i, 'webcal:') };
}

/** Whether the text is a calendar file, as far as the page looks: it opens `BEGIN:VCALENDAR`.
 *  A login page or an error page sent in its place is not saved as one. */
export const isCalendar = (text: string): boolean => /^﻿?\s*BEGIN:VCALENDAR/i.test(text);
