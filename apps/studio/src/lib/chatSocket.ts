// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where the chat's socket opens. A socket cannot send an `Authorization` header, so on a
// station whose own address is behind a login that wants one (Cloudron's proxyAuth) the upgrade
// is turned away with a redirect to the login page. The socket needs no header: its first
// frame (`hello`) carries the chat session, which came from a request that did carry the
// bearer. So the socket goes to the station's public address when it has one, and the requests
// stay on the station's own.

import { publicAddress } from '../schedule/files/links.ts';

/** The addresses to try for the socket, first choice first.
 *
 *  - `chat`: the page's `?chat=` override; it is the only address when given.
 *  - `own`: the address the studio talks to (`base()`); empty when the page's own origin.
 *  - `publicUrl`: the station's public address, if it has one and it was learned.
 *  - `page`: the page's own origin, for when `own` is empty.
 *
 *  A station with no public address, or whose public address is its own, gets its own and
 *  nothing else (a Compose install). Otherwise the public address comes first and the station's
 *  own second, in case the public one cannot be reached from here (split DNS, a firewall). */
export function socketOrigins(o: { chat: string; own: string; publicUrl: string | null | undefined; page: string }): string[] {
  if (o.chat) return [o.chat];
  if (!o.own) return [o.page];
  const pub = publicAddress(o.publicUrl);
  return pub && pub !== o.own.replace(/\/+$/, '') ? [pub, o.own] : [o.own];
}

/** The socket's address on an origin: `https://kettle.example` is `wss://kettle.example/api/v1/chat/socket`. */
export const socketUrlOn = (origin: string): string => `${origin.replace(/\/+$/, '').replace(/^http/, 'ws')}/api/v1/chat/socket`;
