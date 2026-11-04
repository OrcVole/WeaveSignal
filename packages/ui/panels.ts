// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The panel-name registry for the studio app (ADR-0070): every panel's stable name, as its
// `data-panel` attribute, its heading in the user documentation, and the field a bug report
// asks for, with the one line its name plate shows. Copied from the design system's section 8
// (desk B and the screener); a renamed panel keeps its old name as an alias for one release.
// `studio.frame` is the app's own frame (tabs, connection, theme), which the desk B and
// screener pages draw as a mock-up bar; it is in the design system's registry (section 8,
// ADR-0097).

export interface PanelInfo {
  /** `surface.panel[.n]`, lowercase. */
  name: string;
  /** One line on what the panel is, shown on its name plate. */
  what: string;
}

export const DESK_B: readonly PanelInfo[] = [
  { name: 'desk.clock', what: 'The studio clock: room time with tenths, back-timing to the next hard post, and what listeners hear 15 s behind.' },
  { name: 'desk.rundown', what: 'Coming up: the next ten minutes as rows (the post, the handover, the carts they need).' },
  { name: 'desk.playout', what: 'Playout (Liquidsoap): its state chip, the queue, and restart when it is down.' },
  { name: 'desk.stage', what: 'The stage: every source listeners can hear, as tiles, under the lamp and the delay.' },
  { name: 'desk.lamp', what: 'The master lamp: ON AIR, or what listeners hear instead (PLAYOUT DOWN, STANDBY, a fault).' },
  { name: 'desk.delay', what: 'The profanity delay: its length, the arc, DUMPED while it rebuilds, and the two-press dump.' },
  { name: 'desk.tile.1', what: 'Source 1 as a tile: name, state, R128 meter with its scale, fader, and the four keyed buttons.' },
  { name: 'desk.tile.2', what: 'Source 2.' },
  { name: 'desk.tile.3', what: 'Source 3.' },
  { name: 'desk.tile.4', what: 'Source 4.' },
  { name: 'desk.tile.5', what: 'Source 5.' },
  { name: 'desk.tile.6', what: 'Source 6, playout.' },
  { name: 'desk.waiting', what: 'Not on air: the sources that are cued or standing by, with the move rule.' },
  { name: 'desk.audience', what: 'Listeners now, split by mount, and what they hear (all good, the dump, the fallback).' },
  { name: 'desk.picture', what: 'The programme picture: the live two-up or the slate, switched with F7 (ADR-0010).' },
  { name: 'desk.carts', what: "The glance: the top row of the show's cart bank, F1 to F6 from any tab." },
  { name: 'desk.chat', what: "The chat glance in real time, with the screener's private line; the chat itself is a tab away." },
  { name: 'desk.toast', what: 'The reserved slot a toast takes; the chat glance steps aside, so nothing moves and no control is covered.' },
  { name: 'desk.status', what: "The station's health in one line: room, playout, listeners, logger, the talk rule." },
];

export const SCREENER: readonly PanelInfo[] = [
  { name: 'screener.status', what: 'The verdict first, then the show, whether the desk takes callers, listeners, and the two clocks.' },
  { name: 'screener.queue', what: 'Callers from the player page in arrival order: ordinal key, name, town, state, link, topic.' },
  { name: 'screener.screening', what: 'The selected caller: the line for the presenter, the private note, name on air, consent, send to desk.' },
  { name: 'screener.needs', what: 'Needs you: the chat modes, then reports, held messages and notices, each with its keyed actions.' },
  { name: 'screener.studio', what: 'To the studio: private messages only the screener and the presenter see (D40).' },
  { name: 'screener.statement', what: 'The statement of reasons: the action, the reason, the log note, and the words the user receives (D31).' },
  { name: 'screener.room', what: "The room: the chat in real time with the audience's lag stated, the selected message's actions, the staff composer." },
  { name: 'screener.toast', what: 'The reserved toast slot under the Room head, in flow, so a toast moves the feed down and covers nothing.' },
];

export const CARTS: readonly PanelInfo[] = [
  { name: 'carts.wall', what: 'The cart wall: the bank strip and its grid of 24 carts, the left hand of the keyboard (1–6, Q–Y, A–H, Z–N).' },
  { name: 'carts.bank.1', what: 'Bank 1 (⌃F1): its colour is its show’s, everywhere (ADR-0066).' },
  { name: 'carts.bank.2', what: 'Bank 2 (⌃F2).' },
  { name: 'carts.bank.3', what: 'Bank 3 (⌃F3).' },
  { name: 'carts.bank.4', what: 'Bank 4 (⌃F4).' },
  { name: 'carts.bank.5', what: 'Bank 5 (⌃F5).' },
  { name: 'carts.sounding', what: 'What is sounding now, from any bank, and the keys: Space stops the last, ⇧Space twice fades all.' },
];

/** The Schedule tab (calendar.md; ADR-0021, ADR-0045). The design's `calendar.frame` is the
 *  app's own `studio.frame`, so it is not drawn twice; `calendar.log-editor` and
 *  `calendar.card-editor` are new with the build (the design drew neither screen), and
 *  `calendar.show-still` with ADR-0200 (2026-11-03), a field of the show editor. */
export const CALENDAR: readonly PanelInfo[] = [
  { name: 'calendar.toolbar', what: 'Week navigation (← → T), the view, the time zone and its clock change, New show, the legend.' },
  { name: 'calendar.week', what: 'The time-grid week: the axis, seven days, blocks placed by start and length, the now line.' },
  { name: 'calendar.episode-editor', what: "The selected episode: inherited fields marked against fields set here, and the foot's save, next and season actions." },
  { name: 'calendar.series-editor', what: 'The series level: the schedule rule, the hosts, the planned episodes, Draft the season.' },
  { name: 'calendar.aired-episode', what: 'An aired episode, frozen: its values as broadcast and the music-usage line.' },
  { name: 'calendar.log-editor', what: "An automation block's generated hours, item by item with the rule each breaks, another track or none, generate." },
  { name: 'calendar.card-editor', what: 'The card: Poster or Slate, the live preview drawn by the control plane, the alt text, a refused field named.' },
  { name: 'calendar.show-still', what: "The show's own slate picture (ADR-0200): the preview, choose or replace a PNG or JPEG, remove; the slate shows it while the show is on air, else the station's." },
];

export const STUDIO: readonly PanelInfo[] = [
  { name: 'studio.frame', what: "The studio app's frame: the tabs with their keys, the link to the station, the theme." },
];

/** Settings, reached from the frame (station admin, not a numbered tab). */
export const SETTINGS: readonly PanelInfo[] = [
  { name: 'settings.jurisdiction', what: 'The Jurisdiction drop-down: one preset sets every retention period (ADR-0113), and the licence questions.' },
  { name: 'settings.periods', what: 'How long each kind of record is kept, with the rule each period follows and its floor (ADR-0114).' },
  { name: 'settings.holds', what: 'Holds: records a complaint concerns, kept until released, with who, when and why (ADR-0116).' },
];

/** Staff sign-in and accounts (ADR-0127): the sign-in screen, who the window runs as, the
 *  notice when a sign-in ends, and the admin's staff list and time zone in Settings (drawn for
 *  an admin or the station key only, so not in SETTINGS). */
export const ACCOUNT: readonly PanelInfo[] = [
  { name: 'setup.sign-in', what: 'Sign in to a station: its address, a username and password (ADR-0127), or the station key instead.' },
  { name: 'studio.account', what: 'Who this window runs as: name and role, or the station key; sign out and change your password.' },
  { name: 'studio.session-ended', what: 'The sign-in has ended: one way back, Sign in again.' },
  { name: 'settings.staff', what: 'Staff accounts: each person, their username, role and whether the account is on; add, change, turn off, set a password (ADR-0127).' },
  { name: 'settings.tokens', what: "API tokens: a named credential for each integration, limited to the scopes chosen, with an end date and revoking; the secret is shown once when it is made (ADR-0195)." },
  { name: 'settings.zone', what: "The station's one time zone: asked for until someone chooses it, then changed with a warning that every stored show slot and clock hour moves (ADR-0189)." },
];

/** The studio app's own updates (ADR-0015, ADR-0140): a panel of Settings drawn in the packaged
 *  app only (a browser build has no updater), so not in SETTINGS. */
export const UPDATES: readonly PanelInfo[] = [
  { name: 'settings.updates', what: "The studio app's own updates: a newer version with its notes, installed on request and refused while a strip is open; a rollback to the version kept before the last update (ADR-0015, ADR-0140)." },
];

const BY_NAME = new Map([...DESK_B, ...SCREENER, ...CARTS, ...CALENDAR, ...STUDIO, ...SETTINGS, ...ACCOUNT, ...UPDATES].map((p) => [p.name, p]));

/** Every registered name with its line, for the name plates. */
export const PANELS: Readonly<Record<string, string>> = Object.fromEntries([...BY_NAME].map(([k, v]) => [k, v.what]));

/** A panel's registry entry; an unknown name is a bug, so it throws. */
export function panel(name: string): PanelInfo {
  const p = BY_NAME.get(name);
  if (!p) throw new Error(`panel \`${name}\` is not in the registry (packages/ui/panels.ts)`);
  return p;
}
