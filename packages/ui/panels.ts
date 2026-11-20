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
  { name: 'desk.clock', what: 'In the left panel. The studio clock: room time with tenths, back-timing to the next hard post, and when listeners hear it, from the listener delay setting (not measured yet). With a host in their own studio next (ADR-0280) the line is the schedule’s: the post, the grace with Hold, a late host’s count, or who keeps the air. Under it, the handover window (1.2): who is next, where they come from and whether they are ready; a late host and the next one both, the late one first.' },
  { name: 'desk.rundown', what: 'In the left panel. Coming up: the next ten minutes as rows (the post, the handover, the carts they need).' },
  { name: 'desk.playout', what: 'In the left panel. Playout (Liquidsoap): its state chip, the queue, and restart when it is down.' },
  { name: 'desk.stage', what: 'The stage: every source listeners can hear, as tiles, under the lamp and the delay.' },
  { name: 'desk.lamp', what: 'The master lamp: ON AIR, or what listeners hear instead (AUTO, PLAYOUT DOWN, STANDBY, a fault).' },
  { name: 'desk.air', what: 'The button beside the lamp: hand the air to automation, or take it back (ADR-0272).' },
  { name: 'desk.end-show', what: 'The air dialog: End show (⌃⇧E), Hand to automation and Take the air back each ask here, ⏎ to confirm and Esc to keep going.' },
  { name: 'desk.delay', what: 'The profanity delay: its length, the arc, DUMPED while it rebuilds, and the two-press dump.' },
  { name: 'desk.tile.1', what: 'Source 1 as a tile: name, state, R128 meter with its scale, fader, and the four keyed buttons. A host’s own studio (ADR-0280) is a tile too: Own studio, the station’s leveller in place of the fader, whether their monitor link is open, how long its link has held, and Disconnect.' },
  { name: 'desk.tile.2', what: 'Source 2.' },
  { name: 'desk.tile.3', what: 'Source 3.' },
  { name: 'desk.tile.4', what: 'Source 4.' },
  { name: 'desk.tile.5', what: 'Source 5.' },
  { name: 'desk.tile.6', what: 'Source 6, playout.' },
  { name: 'desk.step-note', what: 'The guest’s empty seat, at Solo: why the desk is quiet, and the way up a step (ADR-0271).' },
  { name: 'desk.next', what: 'At Solo, the next show’s source in the guest’s empty seat (ADR-0280): its state, its meter, Cue, and when the schedule brings it on. It has no On key.' },
  { name: 'desk.waiting', what: 'Not on air: the sources that are cued or standing by, with the move rule, and Invite someone…, which makes an invitation. It arrives at the Guests step.' },
  { name: 'desk.invite', what: 'The invitation dialog: a name and a role (a DJ joins off air; a guest or a presenter on air), then the link to send, Copy, and how long it lasts. At Solo it is the DJ’s own microphone link, with Open in browser.' },
  { name: 'desk.audience', what: "In the right panel. Listeners now on the station's stream, by mount, and what they hear (the desk, the dump, the fallback); or why they cannot be counted." },
  { name: 'desk.callers', what: 'Callers, at the Callers step: who is waiting on the phone lines, what about and for how long; take the next caller to strip 4 (P, ADR-0275).' },
  { name: 'desk.picture', what: 'In the right panel. The programme picture: the camera on air or the slate, switched with F7 (ADR-0010).' },
  { name: 'desk.carts', what: "In the right panel. The glance: the top row of the show's cart bank, F1 to F6 from any tab." },
  { name: 'desk.chat', what: 'In the right panel. The chat to read: the room’s last messages, and one line when something waits for a moderator.' },
  { name: 'desk.toast', what: 'The reserved slot a toast takes, above the chat glance: nothing moves and no control is covered. In one column (a narrow window, or large text) it is a row of its own above the status bar.' },
  { name: 'desk.status', what: "The station's health in one line: the room, the link to playout, the listener server, and the talk rule." },
];

export const SCREENER: readonly PanelInfo[] = [
  { name: 'screener.status', what: 'The verdict first, then the show, whether the desk takes callers, listeners, and the two clocks.' },
  { name: 'screener.queue', what: 'Callers from the player page in arrival order: ordinal key, name, town, state, link, topic.' },
  { name: 'screener.screening', what: 'The selected caller: the line for the presenter, the private note, name on air, consent, send to desk.' },
  { name: 'screener.ban-number', what: 'The dialog that bans a phone caller’s number from their card (1.2): why, and for how long. The station keeps the last four digits and a key, never the number.' },
  { name: 'screener.needs', what: "Needs you: the chat modes, then reports, held messages, notices, chatters' pictures waiting for approval, and pictures and documents posted in the chat waiting for approval (ADR-0262), each with its keyed actions." },
  { name: 'screener.studio', what: 'To the studio: private messages only the screener and the presenter see (D40).' },
  { name: 'screener.statement', what: 'The statement of reasons: the action, the reason, the log note, and the words the user receives (D31); for a message, a picture refused or removed, or a picture or document posted in the chat.' },
  { name: 'screener.attachment', what: 'A picture posted in the chat, full size, as the room would see it, so text in a screenshot can be read before deciding (ADR-0262).' },
  { name: 'screener.room', what: "The room: the chat in real time with the audience's lag stated, the selected message's actions, the staff composer." },
  { name: 'screener.toast', what: 'The reserved toast slot under the Room head, in flow, so a toast moves the feed down and covers nothing.' },
];

export const CARTS: readonly PanelInfo[] = [
  { name: 'carts.wall', what: 'The cart wall: the bank strip and its grid of 24 carts, the left hand of the keyboard (1–6, Q–Y, A–H, Z–N).' },
  { name: 'carts.bank.1', what: 'Bank 1 (⌃F1): the show’s own bank 1 if it has one, else the station’s; its colour is its show’s, everywhere (ADR-0066).' },
  { name: 'carts.bank.2', what: 'Bank 2 (⌃F2).' },
  { name: 'carts.bank.3', what: 'Bank 3 (⌃F3).' },
  { name: 'carts.bank.4', what: 'Bank 4 (⌃F4).' },
  { name: 'carts.bank.5', what: 'Bank 5 (⌃F5).' },
  { name: 'carts.sounding', what: 'What is sounding now, from any bank, and the keys: Space stops the last, ⇧Space twice fades all.' },
];

/** The Schedule tab (calendar.md; ADR-0021, ADR-0045). The design's `calendar.frame` is the
 *  app's own `studio.frame`, so it is not drawn twice; `calendar.log-editor` and
 *  `calendar.card-editor` are new with the build (the design drew neither screen), and
 *  `calendar.show-still` with ADR-0200 (2026-11-03), a field of the show editor. The guided
 *  New show and the recurrence rule (ADR-0278, new-show.md section 5) add `calendar.new-show`,
 *  the four `calendar.rule` names, `calendar.show-rule` and `calendar.unscheduled`; Delete show
 *  (ADR-0279) adds `calendar.show-delete`. Week clashes and a show's dates (week-clashes.md
 *  section 6, release 1.0.5) add `calendar.day`, `calendar.day.strip`, `calendar.clash`,
 *  `calendar.shows` and `calendar.show-dates`. Calendar files (ADR-0281, calendar-files.md,
 *  release 1.0.5) add `calendar.subscribe` and `calendar.import`. */
export const CALENDAR: readonly PanelInfo[] = [
  { name: 'calendar.toolbar', what: 'Week navigation (← → T), Week, Day or Studios, the time zone and its clock change, New show, the calendar files (Import a calendar… and Subscribe…: two buttons in a wide window, one menu beside New show in a narrower one), the legend.' },
  { name: 'calendar.week', what: 'The time-grid week: the axis, seven days, blocks placed by start and length, the now line; clashing blocks fold beside one open block; a day’s header opens the day.' },
  { name: 'calendar.day', what: 'One day at the week’s scale: a lane for each clashing show, nothing folded.' },
  { name: 'calendar.day.strip', what: 'Above the Day view’s grid, the week’s seven days as buttons: the open day, today, and each day’s clashes.' },
  { name: 'calendar.clash', what: 'On a block in a clash: the shows it clashes with, Show it on each, who is on air by the schedule, Open the day.' },
  { name: 'calendar.rota', what: 'The rota (1.2), in the calendar’s place: each show in the next twelve hours, who is down for it, where they come from and whether they are ready; the one on air is marked. R on the tab, or Rota beside Coming up on the desk.' },
  { name: 'calendar.shows', what: 'The editor column with nothing open: every show with its rule in short and its next date; the shows with no dates yet, and the finished.' },
  { name: 'calendar.show-dates', what: 'The show level’s Dates tab: the show’s dates as rows (drafted, not drafted, skipped, moved, clashing, aired), with Draft, Skip, Move… and a way to the week.' },
  { name: 'calendar.episode-editor', what: "The selected episode: inherited fields marked against fields set here, and the foot's save, next and season actions." },
  { name: 'calendar.series-editor', what: 'The series level: when it airs, the hosts, the planned episodes, Draft the season.' },
  { name: 'calendar.aired-episode', what: 'An aired episode, frozen: its values as broadcast and the music-usage line.' },
  { name: 'calendar.log-editor', what: "An automation block's generated hours, item by item with the rule each breaks, another track or none, generate." },
  { name: 'calendar.card-editor', what: 'The card: Poster or Slate, the live preview drawn by the control plane, the alt text, a refused field named.' },
  { name: 'calendar.show-still', what: "The show's own slate picture (ADR-0200): the preview, choose or replace a PNG or JPEG, remove; the slate shows it while the show is on air, else the station's." },
  { name: 'calendar.show-step', what: 'The show’s step, with what each one adds: Solo, Guests, Callers or Full. Whatever a step leaves out is switched off while the show is on air (ADR-0271).' },
  { name: 'calendar.new-show', what: 'New show in three parts: the show, when it airs, done (ADR-0278). A dialog over the Schedule tab.' },
  { name: 'calendar.rule', what: 'When a show airs: the first date and times, how it repeats, when it ends. Part 2 of New show, part 3 of the DJ’s setup, and a dialog of its own.' },
  { name: 'calendar.rule.summary', what: 'The rule said back as one sentence, and the last date when it ends. In the preview.' },
  { name: 'calendar.rule.preview', what: 'The next dates in small months and as rows, with clashes, and a date skipped or moved: ↑ ↓ move along the rows, S skips or puts back, M moves.' },
  { name: 'calendar.rule.change', what: 'What a changed rule does to episodes, before the save: aired, drafted, moved by hand.' },
  { name: 'calendar.show-source', what: 'Comes from, in the show editor (ADR-0280): the station’s room or the host’s own studio, with each host’s key and the way to its details.' },
  { name: 'calendar.show-rule', what: 'When it airs, in the show editor: the rule in short, the next date, Change….' },
  { name: 'calendar.show-delete', what: 'Delete show…, from the show editor (ADR-0279): what goes with the show, with its counts, and the two buttons; Keep it is the default. The station refuses a show that has aired.' },
  { name: 'calendar.subscribe', what: 'Subscribe… (ADR-0281): the link to the schedule’s calendar file, or one show’s, on the station’s public address, as webcal and https, with Copy, and Download .ics. A dialog over the Schedule tab.' },
  { name: 'calendar.import', what: 'Import a calendar… in three parts (ADR-0281): choose an .ics file, tick and rename the shows it proposes (each with its sentence, next dates and clashes), done. For an admin or a producer.' },
  { name: 'calendar.unscheduled', what: 'At the foot of the list of shows. Shows with nothing coming up: no dates yet, or finished.' },
  { name: 'calendar.show-presenter', what: "The show's presenter (ADR-0214, ADR-0219): the on-air name and takedown contact the consent words name while it is on air, links to the presenter's channels, and whether it plays commercial music." },
];

export const STUDIO: readonly PanelInfo[] = [
  { name: 'studio.frame', what: "The top bar: the show's step, the tabs with their keys, the app's version, the link to the station, the theme." },
  { name: 'studio.step', what: 'The show’s step as four pips and a word, the way to change it, and Show everything (ADR-0271).' },
  { name: 'studio.step.menu', what: 'The four steps for the show named, what changes now, and the confirm: ↑ ↓ move, ⏎ confirms, Esc keeps the step.' },
  { name: 'studio.step.banner', what: 'Says the whole studio is shown for now and that the show’s step still holds; Back to the step (ADR-0274).' },
  { name: 'studio.prepare', what: 'Whether this window is Live or Preparing, and the way to switch; a preparing window shows the live desk and cannot change what is on air (ADR-0276).' },
  { name: 'studio.prepare.menu', what: 'Live or Preparing for this window, what the switch changes, and the confirm: ↑ ↓ move, ⏎ confirms, Esc keeps what is. The one ask before taking the desk while someone else’s show is on air.' },
  { name: 'studio.prepare.banner', what: 'Says this window is preparing and what is on air, and holds Switch to Live; on the Desk, Screener and Carts tabs (ADR-0276).' },
];

/** Settings, reached from the top bar (station admin, not a numbered tab). */
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
  { name: 'studio.close-ask', what: 'Closing the window while the bridge runs: keep it open, or stop the bridge and close.' },
  { name: 'studio.station', what: 'In the frame: the station this window is on, by its name; it opens the list of stations this computer keeps (ADR-0285).' },
  { name: 'studio.stations', what: 'The stations this computer keeps, each with its own sign-in: switch to one, add one, give one a name here, sign out of one, remove one (ADR-0285).' },
  { name: 'studio.stations.leave', what: 'Asked before this window leaves a station that is on air: what stays on there, then Stay or Switch (ADR-0285).' },
  { name: 'setup.stations', what: 'On the sign-in screen: the other stations this computer keeps, to open one instead (ADR-0285).' },
  { name: 'settings.staff', what: 'Staff accounts: each person, their username, role and whether the account is on; add, change, turn off, set a password (ADR-0127).' },
  { name: 'settings.staff.add', what: "Adding a member of staff: their username, name, role and first password." },
  { name: 'settings.staff.off', what: "Turning an account off, confirmed: its sessions end at once." },
  { name: 'settings.staff.password', what: "Setting a member of staff's password." },
  { name: 'settings.staff.email', what: "A member of staff's email address, for signing in by an emailed code." },
  { name: 'settings.hosts', what: 'Own studios (ADR-0280): each show key, who it is for, how they send and whether they are connected; set up a host, see the details, make a new key, end one, let a key back in.' },
  { name: 'settings.hosts.rules', what: 'The station’s rules for hosts in their own studios: how early a host may connect, tests outside the slot, the grace and the late count, and the two ports.' },
  { name: 'settings.hosts.setup', what: 'Setting up a host: the show, who it is for, how they will send.' },
  { name: 'settings.hosts.details', what: 'A host’s connection details for Icecast and SRT and their monitor link; the secret, shown this once, with Copy on each field and Copy everything as text.' },
  { name: 'settings.hosts.new-key', what: 'Making a new key, confirmed: the old one ends now, or when the host disconnects.' },
  { name: 'settings.hosts.end', what: 'Ending a key, confirmed: what stops, and what listeners hear if the host is on air.' },
  { name: 'settings.phone', what: 'The phone line (1.2): whether it takes calls and what puts it right, whether the consent words play and which voice a caller hears (Alba, Edinburgh or the station’s own recording of them), the station’s takedown contact, and the banned numbers with Ban a number and Find a number’s calls. An admin’s.' },
  { name: 'settings.tokens', what: "API tokens: a named credential for each integration, limited to the scopes chosen, with an end date and revoking; the secret is shown once when it is made (ADR-0195)." },
  { name: 'settings.tokens.make', what: "Making an API token: its name, scopes and end date." },
  { name: 'settings.tokens.secret', what: "A new token's secret, shown this once, with Copy." },
  { name: 'settings.tokens.revoke', what: "Revoking an API token, confirmed: it stops working at once." },
  { name: 'settings.zone', what: "The station's one time zone: asked for until someone chooses it, then changed with a warning that every stored show slot and clock hour moves (ADR-0189)." },
  { name: 'settings.chat-pictures', what: "Chat pictures: the switch that turns chatters' uploaded pictures on and off at once, the install setting being its default until it is used (ADR-0259); the switch for pictures and documents posted in the chat by trusted chatters (ADR-0262); who sees a picture kept as evidence of suspected child sexual abuse material; and the pictures the station keeps out of view, each deletable at once with why (ADR-0257)." },
  { name: 'settings.chat-pictures.delete', what: "Deleting a kept picture from the server at once: why, for the moderation record; for evidence of suspected child sexual abuse material, the law's words and a confirmation (ADR-0257)." },
  { name: 'settings.chat-pictures.look', what: "A picture kept as evidence, shown to the one named person who may see it (ADR-0257)." },
  { name: 'settings.icecast-out', what: "The programme sent to another Icecast server, such as a DJ's AzuraCast: whether it is connected, the last error, and when it sends - while the desk is on air, always, or never (ADR-0226)." },
  { name: 'settings.bridge', what: "The bridge (Phase 17g): this computer stands in for the station, so a host's own software sends to it on loopback behind a local password, and the studio app carries it to the station over one connection on 443; the station's public address, the show key kept in the keychain, the local addresses to copy, and the uplink's state with the desk's answer." },
  { name: 'settings.kit', what: "The presenter kit's station half (Phase 17g): VBAN in bound to show keys, VBAN out from the desk, the presets, whether the installation allows VBAN at all (never on Cloudron), and the one clock's figures." },
];

/** The DJ's show setup (ADR-0218, ADR-0219): the wizard over the surface, and its panel in
 *  Settings for a member of staff signed in as themselves (so not in SETTINGS). */
export const SHOW_SETUP: readonly PanelInfo[] = [
  { name: 'studio.show-setup', what: 'Set up your show, in four parts: your on-air name and takedown contact; your show, its links, whether it plays commercial music and how much of the studio to start with; when it is on, with the next dates; done. The answers are the defaults for you and your show (ADR-0218).' },
  { name: 'settings.my-show', what: 'Your show: the show tied to your account as the setup wizard left it (name, on-air name, takedown contact, links, when it is on, commercial music), with Change… to run the wizard again; or Set up your show… (ADR-0218).' },
];

/** The studio app's own updates (ADR-0015, ADR-0140): a panel of Settings drawn in the packaged
 *  app only (a browser build has no updater), so not in SETTINGS. */
export const UPDATES: readonly PanelInfo[] = [
  { name: 'settings.updates', what: "The studio app's own updates: a newer version with its notes, installed on request and refused while a strip is open; a rollback to the version kept before the last update (ADR-0015, ADR-0140)." },
];

/** The website's sandbox desk (ADR-0212) and its training scenarios (ADR-0222): the bar over
 *  the app's frame and the cards around a scenario, drawn in the sandbox build only. */
export const SANDBOX: readonly PanelInfo[] = [
  { name: 'sandbox.guide', what: "The sandbox's guide bar: the scenario's next step, read from the simulated station; Learn, the way to the other scenarios; the stand-in tab; Hear (the studio or the listeners) (ADR-0212, ADR-0222)." },
  { name: 'sandbox.steps', what: 'Learn: the four steps a show can run at; each opens its scenarios, and the running scenario’s step is lit (ADR-0271).' },
  { name: 'sandbox.tab', what: 'A stand-in for the browser tab a DJ speaks through beside the studio app (the app does not join the room itself yet, ADR-0013): whose tab, what it is doing, and its microphone (ADR-0222).' },
  { name: 'sandbox.caption', what: 'The caption for the line being said: who, where (the booth or on air), the words; marked when the voice is not recorded yet (ADR-0222).' },
  { name: 'sandbox.chooser', what: 'The scenarios as four rungs, Solo at the top, one job or one skill each, with how long each takes; picking one starts the station afresh, at that scenario’s step (ADR-0222, ADR-0271).' },
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
