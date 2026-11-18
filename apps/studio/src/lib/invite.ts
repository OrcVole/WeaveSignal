// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The desk's invitation (ADR-0074): the words of its dialog, kept out of the component so the
// tests can hold them. An invitation is a link to the station's join page with a pass in its
// fragment; the role decides whether its person joins on air or off. The show's step decides
// which roles there are (ADR-0271): the next show's DJ comes with Full.

import { FieldError, type Invite, type InviteRole, type Me, type ShowStep, type Source, type View } from './api.ts';
import { firstName, hm } from './format.ts';
import { atLeast, needsOwnLink } from './steps.ts';

/** The roles an invitation may carry, the DJ first (the default), each with what joining means. */
export const INVITE_ROLES: readonly { role: InviteRole; name: string; what: string }[] = [
  { role: 'dj', name: 'DJ', what: 'Joins off air, under Not on air. Listeners hear nothing of them until you put them on. For the next show’s DJ.' },
  { role: 'guest', name: 'Guest', what: 'On air the moment they join.' },
  { role: 'presenter', name: 'Presenter', what: 'On air the moment they join, as a presenter of this show.' },
];

/** The roles the station's step lets the desk invite: a guest and a presenter from Guests, the
 *  next show's DJ at Full (the station refuses the rest). The first is the one picked. */
export function rolesAt(step: ShowStep): readonly { role: InviteRole; name: string; what: string }[] {
  return INVITE_ROLES.filter((r) => r.role !== 'dj' || atLeast(step, 'full'));
}

/** Under the roles while the DJ is not among them. */
export const DJ_COMES_WITH_FULL = 'An invitation for the next show’s DJ comes with Full.';

/** What is wrong with the name as typed, or null: the desk shows it on their tile. */
export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Enter their name: the desk shows it on their tile.';
  if ([...n].length > 120) return 'Use a shorter name: 120 characters at most.';
  return null;
}

/** The whole link, to send: the join page lives at the station's own address. `origin` is the
 *  station's address as this window knows it (`base()`), or the page's own. */
export function inviteUrl(link: string, origin: string): string {
  return /^https?:\/\//.test(link) ? link : `${origin.replace(/\/$/, '')}${link}`;
}

/** `4 hours`, `1 hour 30 minutes`, `45 minutes`: how long a link lasts, to the minute. */
export function lasts(ms: number): string {
  const min = Math.max(1, Math.round(ms / 60_000));
  const h = Math.floor(min / 60);
  const m = min % 60;
  const hours = h ? `${h} ${h === 1 ? 'hour' : 'hours'}` : '';
  const minutes = m ? `${m} ${m === 1 ? 'minute' : 'minutes'}` : '';
  return [hours, minutes].filter(Boolean).join(' ');
}

/** How long the link lasts and when it ends, by this machine's clock. */
export function lastsLine(invite: Pick<Invite, 'expires_at_ms'>, now: number): string {
  return `The link works for ${lasts(invite.expires_at_ms - now)}, until ${hm(invite.expires_at_ms)}.`;
}

/** What happens when they open it, by role. */
export function joinsLine(invite: Pick<Invite, 'role' | 'name' | 'identity'>): string {
  const who = firstName(invite.name, invite.identity);
  return invite.role === 'dj'
    ? `${who} opens it in a browser and presses Join the studio. They arrive off air, under Not on air.`
    : `${who} opens it in a browser and presses Join the studio. They are on air the moment they join.`;
}

/** A sentence of the station's own, whole: it is shown as it stands (a step's refusal says
 *  what is off and where it comes back, and nothing of ours improves on it). */
const whole = (m: string): boolean => /^\p{Lu}.*[.!?]$/su.test(m.trim());

/** A refusal in words: the station's own sentence where it has one worth reading, ours where
 *  it is a status or the network. */
export function inviteProblem(e: unknown): string {
  if (e instanceof FieldError) {
    if (e.status === 401) return 'Your sign-in has ended. Sign in again, then create the invitation.';
    if ((e.status === 403 || e.status === 409 || e.status === 422) && whole(e.message)) return e.message.trim();
    if (e.status === 403) return 'Your account may not create invitations. Ask a station admin.';
    if (e.status === 503) return `This station cannot create invitations yet. It says: ${e.message}.`;
    if ((e.status === 422 || e.status === 400) && e.message && !/^\d{3}\b/.test(e.message)) return `The station refused the invitation. It says: ${e.message}.`;
    return 'The station could not create the invitation. Try again.';
  }
  return 'The station did not answer. Check the link to the station, then try again.';
}

/** The identity the station gives a presenter's own microphone link: `presenter-me-` and the
 *  account's id without its hyphens (`crates/control/src/steps.rs`, `own_identity`). */
export const ownIdentity = (accountId: string) => `presenter-me-${accountId.replaceAll('-', '')}`;

/** Whether a person at the desk is this window's own account in the room, by its microphone link. */
export function isMine(identity: string, me: Me | null | undefined): boolean {
  return me?.kind === 'staff' && !!me.account && identity === ownIdentity(me.account.id);
}

/** Leaving the room from the desk (the operator on talk, 2026-11-16: the only way off the air was
 *  the join page's own Leave). The presenter's own tile says it, and Leave is End pass for
 *  yourself, in two presses like it. */
export const LEAVE = {
  button: 'Leave',
  inRoom: 'You’re in the room',
  why: 'Takes you off air and out of the room, and closes your microphone link. A new microphone link brings you back.',
  doneTitle: 'You have left the room',
} as const;

/** What the toast says after Leave, from the view the station answered with (as `letGoWords`
 *  does after Off air): where the air is, and how to come back only where that button is drawn. */
export function leaveWords(v: View | null | undefined, signedIn: boolean, look: ShowStep): { title: string; message: string } {
  const onAir = (v?.sources ?? []).filter((s) => s.on_air).map((s) => firstName(s.name, s.identity));
  const who = onAir.length > 1 ? `${onAir.slice(0, -1).join(', ')} and ${onAir[onAir.length - 1]}` : (onAir[0] ?? '');
  const air = onAir.length
    ? `${who} ${onAir.length === 1 ? 'keeps' : 'keep'} the air.`
    : v?.mode === 'auto'
      ? 'Automation holds the air.'
      : v?.mode === null || v?.mode === undefined
        ? 'Listeners hear the fallback.'
        : 'Nobody is on air.';
  const back = v && needsOwnLink(v.sources, look, !!v.step?.may_change, signedIn) ? ' Join with my microphone… makes the link again.' : '';
  return { title: LEAVE.doneTitle, message: `You are off air and your microphone link is closed. ${air}${back}` };
}

/** The Solo DJ's own link (ADR-0274): the lines of the dialog's second page. */
export const OWN_LINK = {
  button: 'Join with my microphone…',
  title: 'Your microphone link',
  lede: 'Open it in a browser on this computer and press Join the studio. You are on air the moment you join. Keep that tab open: it carries your voice.',
  /** The lede while automation holds the air. */
  ledeAuto: 'Open it in a browser on this computer and press Join the studio. Automation holds the air, so you join open to the room; Take the air back puts you on air. Keep that tab open: it carries your voice.',
  /** The stage's words before the link is made: the tab is the microphone. */
  stage: 'Your voice reaches the desk from a browser tab on this computer. Open your microphone link, press Join the studio, and you are on air. Keep that tab open: it carries your voice.',
  /** The same while automation holds the air: joining opens you to the room, and the air stays
   *  automation's until it is taken back (the post's rule, ADR-0280). */
  stageAuto: 'Your voice reaches the desk from a browser tab on this computer. Open your microphone link and press Join the studio: you join open to the room while automation holds the air, and Take the air back puts you on air. Keep that tab open: it carries your voice.',
  open: 'Open in browser',
  /** After the link is handed to the browser. */
  opened: 'Opened in your browser. Allow the microphone if it asks, then press Join the studio. Keep that tab open: it carries your voice.',
  /** The packaged app could not hand the link to a browser. */
  notOpened: 'Could not open a browser from here. Copy the link and paste it into your browser.',
} as const;

/** The host in their own studio who has the air (1.1): a source on air, or one whose place is
 *  kept while its link is lost, since the air is theirs to come back to. `name` is their first
 *  name, empty when two hosts are on air at once (an overlap). Null when no source holds the
 *  air: the stage's offer is then the headline it always was. */
export interface AirHost {
  name: string;
  /** Their link is lost and their place is kept: nobody is there to talk to. */
  lost: boolean;
}

export function airHost(sources: readonly Pick<Source, 'role' | 'on_air' | 'name' | 'identity'>[], kept: { name: string } | null | undefined): AirHost | null {
  const on = sources.filter((s) => s.role === 'source' && s.on_air);
  if (on.length) return { name: on.length === 1 ? firstName(on[0].name, on[0].identity) : '', lost: false };
  return kept ? { name: firstName(kept.name, ''), lost: true } : null;
}

/** The offer's one quiet line while a host in their own studio has the air (the look's review,
 *  2026-11-17, finding 2): beside Invite someone… in the Not on air row, not the stage's
 *  headline. The show is not this window's to present, so the line says what joining is for,
 *  Talk and Cue, and that joining puts you on air: here that is a warning, not a promise.
 *
 *  It says only what the station will do (`plan` in crates/model/src/post.rs). While automation
 *  holds the air (`mode` is `auto`: it covers a lost link, or was handed the air) a presenter
 *  who joins is open to the room and not heard, so the line says that and not *on air* (the
 *  lamp and the stage's own line above say automation holds the air; one line has no room to
 *  say it again). With the
 *  host's link lost there is nobody to talk to. Then, under automation, the station brings a
 *  returning host back by itself and takes the air back with them, which puts whoever joined on
 *  air too; with the desk holding the air, whoever joined holds it, and the returning host
 *  stands by. */
export function ownLinkBeside(host: AirHost, mode?: View['mode']): string {
  const auto = mode === 'auto';
  if (host.lost) {
    const who = host.name || 'the host';
    return auto ? `You join open to the room, and are on air when ${who} is back.` : `Joining puts you on air. If ${who} reconnects, they then stand by.`;
  }
  const who = host.name || 'the hosts';
  return `Join to talk to ${who} or to hear a cue. ${auto ? 'You join open to the room.' : 'Joining puts you on air.'}`;
}
