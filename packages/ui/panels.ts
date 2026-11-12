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
  { name: 'desk.clock', what: 'The studio clock: room time with tenths, back-timing to the next hard post, and when listeners hear it, from the listener delay setting (not measured yet).' },
  { name: 'desk.rundown', what: 'Coming up: the next ten minutes as rows (the post, the handover, the carts they need).' },
  { name: 'desk.playout', what: 'Playout (Liquidsoap): its state chip, the queue, and restart when it is down.' },
  { name: 'desk.stage', what: 'The stage: every source listeners can hear, as tiles, under the lamp and the delay.' },
  { name: 'desk.lamp', what: 'The master lamp: ON AIR, or what listeners hear instead (AUTO, PLAYOUT DOWN, STANDBY, a fault).' },
  { name: 'desk.delay', what: 'The profanity delay: its length, the arc, DUMPED while it rebuilds, and the two-press dump.' },
  { name: 'desk.tile.1', what: 'Source 1 as a tile: name, state, R128 meter with its scale, fader, and the four keyed buttons.' },
  { name: 'desk.tile.2', what: 'Source 2.' },
  { name: 'desk.tile.3', what: 'Source 3.' },
  { name: 'desk.tile.4', what: 'Source 4.' },
  { name: 'desk.tile.5', what: 'Source 5.' },
  { name: 'desk.tile.6', what: 'Source 6, playout.' },
  { name: 'desk.waiting', what: 'Not on air: the sources that are cued or standing by, with the move rule, and Invite someone….' },
  { name: 'desk.invite', what: 'The invite dialog: a name and a role (a DJ joins off air; a guest or a presenter on air), then the link to send, Copy, and how long it lasts.' },
  { name: 'desk.audience', what: "Listeners now on the station's stream, and what they hear (the desk, the dump, the fallback); counts by mount are not shown yet." },
  { name: 'desk.picture', what: 'The programme picture: the camera on air or the slate, switched with F7 (ADR-0010).' },
  { name: 'desk.carts', what: "The glance: the top row of the show's cart bank, F1 to F6 from any tab." },
  { name: 'desk.chat', what: "Where the chat glance will go; the chat is on the Screener tab for now." },
  { name: 'desk.toast', what: 'The reserved slot a toast takes; the chat glance steps aside, so nothing moves and no control is covered.' },
  { name: 'desk.status', what: "The station's health in one line: the room, the link to playout, the listener server, and the talk rule." },
];

export const SCREENER: readonly PanelInfo[] = [
  { name: 'screener.status', what: 'The verdict first, then the show, whether the desk takes callers, listeners, and the two clocks.' },
  { name: 'screener.queue', what: 'Callers from the player page in arrival order: ordinal key, name, town, state, link, topic.' },
  { name: 'screener.screening', what: 'The selected caller: the line for the presenter, the private note, name on air, consent, send to desk.' },
  { name: 'screener.needs', what: "Needs you: the chat modes, then reports, held messages, notices, chatters' pictures waiting for approval, and pictures and documents posted in the chat waiting for approval (ADR-0262), each with its keyed actions." },
  { name: 'screener.studio', what: 'To the studio: private messages only the screener and the presenter see (D40).' },
  { name: 'screener.statement', what: 'The statement of reasons: the action, the reason, the log note, and the words the user receives (D31); for a message, a picture refused or removed, or a picture or document posted in the chat.' },
  { name: 'screener.attachment', what: 'A picture posted in the chat, full size, as the room would see it, so text in a screenshot can be read before deciding (ADR-0262).' },
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
  { name: 'calendar.show-presenter', what: "The show's presenter (ADR-0214, ADR-0219): the on-air name and takedown contact the consent words name while it is on air, links to the presenter's channels, and whether it plays commercial music." },
];

export const STUDIO: readonly PanelInfo[] = [
  { name: 'studio.frame', what: "The studio app's frame: the tabs with their keys, the link to the station, the theme." },
];

/** Settings, reached from the frame (station admin, not a numbered tab). */
export const SETTINGS: readonly PanelInfo[] = [
  { name: 'settings.jurisdiction', what: 'The Jurisdiction drop-down: one preset sets every retention period (ADR-0113), and the licence questions.' },
  { name: 'settings.periods', what: 'How long each kind of record is kept, with the rule each period follows and its floor (ADR-0114).' },
  { name: 'settings.periods.confirm', what: "Shortening a retention period, confirmed: what the next sweep will delete." },
  { name: 'settings.holds', what: 'Holds: records a complaint concerns, kept until released, with who, when and why (ADR-0116).' },
  { name: 'settings.holds.extend', what: "Extending a hold: to when, and why." },
  { name: 'settings.holds.release', what: "Releasing a hold, with why: its records then follow their periods again." },
];

/** Staff sign-in and accounts (ADR-0127): the sign-in screen, who the window runs as, the
 *  notice when a sign-in ends, and the admin's staff list and time zone in Settings (drawn for
 *  an admin or the station key only, so not in SETTINGS). */
export const ACCOUNT: readonly PanelInfo[] = [
  { name: 'setup.sign-in', what: 'Sign in to a station: its address, a username and password (ADR-0127), or the station key instead.' },
  { name: 'studio.account', what: 'Who this window runs as: name and role, or the station key; sign out and change your password.' },
  { name: 'studio.session-ended', what: 'The sign-in has ended: one way back, Sign in again.' },
  { name: 'settings.staff', what: 'Staff accounts: each person, their username, role and whether the account is on; add, change, turn off, set a password (ADR-0127).' },
  { name: 'settings.staff.add', what: "Adding a member of staff: their username, name, role and first password." },
  { name: 'settings.staff.off', what: "Turning an account off, confirmed: its sessions end at once." },
  { name: 'settings.staff.password', what: "Setting a member of staff's password." },
  { name: 'settings.staff.email', what: "A member of staff's email address, for signing in by an emailed code." },
  { name: 'settings.tokens', what: "API tokens: a named credential for each integration, limited to the scopes chosen, with an end date and revoking; the secret is shown once when it is made (ADR-0195)." },
  { name: 'settings.tokens.make', what: "Making an API token: its name, scopes and end date." },
  { name: 'settings.tokens.secret', what: "A new token's secret, shown this once, with Copy." },
  { name: 'settings.tokens.revoke', what: "Revoking an API token, confirmed: it stops working at once." },
  { name: 'settings.zone', what: "The station's one time zone: asked for until someone chooses it, then changed with a warning that every stored show slot and clock hour moves (ADR-0189)." },
  { name: 'settings.chat-pictures', what: "Chat pictures: the switch that turns chatters' uploaded pictures on and off at once, the install setting being its default until it is used (ADR-0259); the switch for pictures and documents posted in the chat by trusted chatters (ADR-0262); who sees a picture kept as evidence of suspected child sexual abuse material; and the pictures the station keeps out of view, each deletable at once with why (ADR-0257)." },
  { name: 'settings.chat-pictures.delete', what: "Deleting a kept picture from the server at once: why, for the moderation record; for evidence of suspected child sexual abuse material, the law's words and a confirmation (ADR-0257)." },
  { name: 'settings.chat-pictures.look', what: "A picture kept as evidence, shown to the one named person who may see it (ADR-0257)." },
  { name: 'settings.icecast-out', what: "The programme sent to another Icecast server, such as a DJ's AzuraCast: whether it is connected, the last error, and when it sends - while the desk is on air, always, or never (ADR-0226)." },
];

/** The DJ's show setup (ADR-0218, ADR-0219): the wizard over the surface, and its panel in
 *  Settings for a member of staff signed in as themselves (so not in SETTINGS). */
export const SHOW_SETUP: readonly PanelInfo[] = [
  { name: 'studio.show-setup', what: 'Set up your show, in three steps: your on-air name and takedown contact; your show, its links, usual time and whether it plays commercial music; done. The answers are the defaults for you and your show (ADR-0218).' },
  { name: 'settings.my-show', what: 'Your show: the show tied to your account as the setup wizard left it (name, on-air name, takedown contact, links, usual time, commercial music), with Change… to run the wizard again; or Set up your show… (ADR-0218).' },
];

/** The studio app's own updates (ADR-0015, ADR-0140): a panel of Settings drawn in the packaged
 *  app only (a browser build has no updater), so not in SETTINGS. */
export const UPDATES: readonly PanelInfo[] = [
  { name: 'settings.updates', what: "The studio app's own updates: a newer version with its notes, installed on request and refused while a strip is open; a rollback to the version kept before the last update (ADR-0015, ADR-0140)." },
];

/** The website's sandbox desk (ADR-0212) and its training scenarios (ADR-0222): the bar over
 *  the app's frame and the cards around a scenario, drawn in the sandbox build only. */
export const SANDBOX: readonly PanelInfo[] = [
  { name: 'sandbox.guide', what: "The sandbox's guide bar: the scenario's next step, read from the simulated station; the stand-in tab; Hear (the studio or the listeners); the way to the other scenarios (ADR-0212, ADR-0222)." },
  { name: 'sandbox.tab', what: 'A stand-in for the browser tab a DJ speaks through beside the studio app (the app does not join the room itself yet, ADR-0013): whose tab, what it is doing, and its microphone (ADR-0222).' },
  { name: 'sandbox.caption', what: 'The caption for the line being said: who, where (the booth or on air), the words; marked when the voice is not recorded yet (ADR-0222).' },
  { name: 'sandbox.chooser', what: 'The scenarios: the show first, then one job or one skill each, with how long each takes; picking one starts the station afresh (ADR-0222).' },
  { name: 'sandbox.start', what: 'A scenario’s start card: whose chair you sit in, what you will learn, the two windows a DJ really uses, and the press that starts the sound (ADR-0222).' },
  { name: 'sandbox.end', what: 'A scenario’s end card: what you did, as the station recorded it; the other scenarios, run it again, or keep playing (ADR-0222).' },
];

/** rtmp-out: RTMP out (ADR-0230), a panel of Settings drawn for an admin or the station key
 *  only, so not in SETTINGS. */
export const RTMP_OUT: readonly PanelInfo[] = [
  { name: 'settings.rtmp', what: "RTMP out: each destination (Rumble, Odysee, PeerTube or any RTMP server) with its server URL, its formats and whether it is live; add, change, turn on or off, remove. The stream key is sent once and never shown (ADR-0230)." },
  { name: 'settings.rtmp.destination', what: "RTMP out's destination dialog: the platform, the server URL, the stream key (sent once, never shown) and the formats." },
  { name: 'settings.rtmp.remove', what: "Removing an RTMP destination, confirmed: its stream key is deleted with it." },
];

const BY_NAME = new Map([...DESK_B, ...SCREENER, ...CARTS, ...CALENDAR, ...STUDIO, ...SETTINGS, ...ACCOUNT, ...SHOW_SETUP, ...UPDATES, ...SANDBOX, ...RTMP_OUT].map((p) => [p.name, p]));

/** Every registered name with its line, for the name plates. */
export const PANELS: Readonly<Record<string, string>> = Object.fromEntries([...BY_NAME].map(([k, v]) => [k, v.what]));

/** A panel's registry entry; an unknown name is a bug, so it throws. */
export function panel(name: string): PanelInfo {
  const p = BY_NAME.get(name);
  if (!p) throw new Error(`panel \`${name}\` is not in the registry (packages/ui/panels.ts)`);
  return p;
}
