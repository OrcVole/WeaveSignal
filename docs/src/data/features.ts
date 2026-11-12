// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What WeaveSignal can do, grouped by what a station cares about, for /features/ and the home
// page's short list (`lead`, the entries marked `home`). Every feature here is in 1.0.1:
// check a new one against "What release one does, and what it leaves out" before adding it, and
// put anything planned in `next`, never here. `clip` names a moment from the demo video
// (public/media/moment-<clip>.webm, present only at a build that has it) and `shot` a screenshot
// the page imports; `what` is the benefit for the station in one or two sentences, `href` its
// documentation page.

export interface Feature {
  id: string;
  name: string;
  what: string;
  href: string;
  link: string;
  home?: string;
  clip?: 'on-air' | 'guest-joins' | 'call-in' | 'dump' | 'playout-down';
  shot?: string;
}

export interface Group {
  id: string;
  title: string;
  lead: string;
  features: Feature[];
}

export const groups: Group[] = [
  {
    id: 'live',
    title: 'Live shows with guests',
    lead: 'A presenter and remote guests talk in one room, from a browser, and hear each other as if they were in the studio.',
    features: [
      {
        id: 'mix-minus',
        name: 'A mix of their own for everyone',
        what: 'The desk sends each person the show without their own voice, so nobody hears themselves back late and no station needs a hybrid or a second mixer. A guest\'s return arrives 69–77 ms after the voice on one machine.',
        href: '/docs/reference/who-hears-whom/',
        link: 'Who hears whom',
        home: 'The desk sends each person the show without their own voice, so nobody hears themselves back late and nobody needs a hybrid.',
        clip: 'guest-joins',
        shot: 'guest',
      },
      {
        id: 'join-from-a-link',
        name: 'Guests join from a link',
        what: 'A guest opens the invite in a browser on a computer or a phone: no account, no app. A network that blocks WebRTC reaches the room through the station\'s own relay, over TLS.',
        href: '/docs/how-to/get-ready/',
        link: 'Get ready to present',
      },
      {
        id: 'cue-talk-mute',
        name: 'Cue, talk, mute and talkback',
        what: 'Hear a source in your headphones before it goes to air, talk to one person without listeners hearing, and count a guest in; each on a key, and a cued or muted source never moves on the screen.',
        href: '/docs/how-to/count-in/',
        link: 'Start on the count-in',
      },
      {
        id: 'voice-chain',
        name: 'A voice chain on every presenter',
        what: 'Each presenter\'s and DJ\'s voice goes through a gate, a compressor, a leveller and a limiter set in the station\'s settings, and every mix has a limiter of its own, so a loud guest cannot clip the programme.',
        href: '/docs/reference/settings/',
        link: 'Settings',
      },
      {
        id: 'echo',
        name: 'Echo handled, headphones explained',
        what: 'The browser cancels echo and the desk gates the quiet, so a presenter\'s microphone does not put a guest back into the programme a third of a second late. The documentation says why headphones matter, in words for a guest.',
        href: '/docs/explanation/echo-and-headphones/',
        link: 'Why headphones',
      },
    ],
  },
  {
    id: 'callers',
    title: 'Callers and screening',
    lead: 'Listeners call from the station\'s own page, wait in private, and reach the presenter through a screener.',
    features: [
      {
        id: 'call-in',
        name: 'Callers from the station\'s own page',
        what: 'A listener presses Call the studio on the player page and talks through the browser on their phone or computer: no telephone number, no app, no account. A real caller rang a public station this way from a phone.',
        href: '/docs/reference/public-pages/',
        link: 'The public pages',
        home: 'A listener presses Call the studio on the station\'s page and waits in a booth of their own until the screener answers; no telephone number, no app.',
        clip: 'call-in',
        shot: 'screener',
      },
      {
        id: 'booths',
        name: 'Each caller in a private booth',
        what: 'A caller waits in a room of their own, hearing the show, seen and heard by nobody until the screener picks them up. Callers never hear each other, and a guest never hears a caller who was not sent to the desk.',
        href: '/docs/reference/who-hears-whom/#what-a-waiting-caller-hears',
        link: 'What a waiting caller hears',
      },
      {
        id: 'screener',
        name: 'A screener between the callers and the desk',
        what: 'The screener talks to each caller off air, writes the one line the presenter sees, and sends them to the desk. A caller banned before is marked in the queue before anyone hears them.',
        href: '/docs/how-to/screen-callers/',
        link: 'Screen callers',
        shot: 'screener',
      },
      {
        id: 'to-air',
        name: 'To air with the screener\'s line under their name',
        what: 'The presenter sees the caller\'s name on air and the screener\'s line, cues them, takes them to air with one key and drops them with two presses. When the desk cannot take a caller now, the screener sees DESK BUSY.',
        href: '/docs/how-to/take-a-caller/',
        link: 'Take a caller to air',
      },
      {
        id: 'consent',
        name: 'Consent and a proof of work, not a CAPTCHA',
        what: 'Every caller agrees to be recorded before the station issues them a token, in two sentences with a takedown contact. The page\'s challenge makes a flood of calls expensive for a program without a puzzle for a person.',
        href: '/docs/reference/public-pages/#call-ins-challenge',
        link: 'Call-in\'s challenge',
      },
      {
        id: 'telephone-sound',
        name: 'Callers sound like a phone line',
        what: 'A caller\'s voice is given a telephone sound by default, so listeners tell the caller from the presenter at once; it is one setting to turn off.',
        href: '/docs/reference/settings/',
        link: 'Settings',
      },
    ],
  },
  {
    id: 'on-air',
    title: 'Staying on air',
    lead: 'A delay to dump into, a lamp that tells the truth, and restarts that disconnect nobody.',
    features: [
      {
        id: 'delay-and-dump',
        name: 'A profanity delay with Dump',
        what: 'The programme is held before it leaves the studio, 8 s by default. Del Del throws the last seconds away: listeners never hear the swear, and the room goes on talking, because the delay sits after the desk and never in the room.',
        href: '/docs/reference/states/#the-delay',
        link: 'The delay',
        home: 'The programme is held 8 s before it leaves the studio; Del Del throws the last seconds away and listeners never hear the swear.',
        clip: 'dump',
        shot: 'dumped',
      },
      {
        id: 'lamp',
        name: 'A lamp that tells the truth',
        what: 'ON AIR lights only when listeners can hear you. OPEN, AUTO, STANDBY and PLAYOUT DOWN each mean one thing, and every state word on the desk has a sentence that says what it promises.',
        href: '/docs/explanation/on-air/',
        link: 'What ON AIR promises',
        clip: 'on-air',
        shot: 'on-air',
      },
      {
        id: 'restarts',
        name: 'A restart disconnects nobody',
        what: 'The listener process holds the stream and keeps a cushion for every player. Playout and the desk can stop or restart underneath it: players hear the fallback for a few seconds, then the programme again, and are never disconnected.',
        href: '/docs/explanation/architecture/#why-the-listener-process-holds-the-mount',
        link: 'Why the listener process holds the mount',
        home: 'The listener process holds the stream, so playout and the desk can restart underneath it and every player stays connected.',
        clip: 'playout-down',
        shot: 'playout-down',
      },
      {
        id: 'dead-air',
        name: 'Dead air is filled',
        what: 'When nothing is on air the listener process plays the station\'s fallback files, and a shipped default loop when there are none, so a new station proves itself before a single file is uploaded.',
        href: '/docs/reference/playout/#the-default-loop',
        link: 'The default loop',
      },
      {
        id: 'trouble-named',
        name: 'Trouble named before it is heard',
        what: 'A tile says LINK DEGRADED when a person\'s connection stalls, the screener\'s queue says DESK BUSY, the toast says what to press, and the fifteen state words never mean two things.',
        href: '/docs/reference/states/',
        link: 'The state vocabulary',
      },
      {
        id: 'one-persons-trouble',
        name: 'One person\'s trouble stays theirs',
        what: 'A guest whose voice fails is resubscribed and a failed mix is rebuilt while the programme carries on; only the programme itself and its link to playout can stop the desk.',
        href: '/docs/explanation/architecture/#when-a-part-stops',
        link: 'When a part stops',
      },
    ],
  },
  {
    id: 'playout',
    title: 'Playout and the schedule',
    lead: 'Automation fills the air between shows, from a week planned on a grid.',
    features: [
      {
        id: 'clocks',
        name: 'Automation from clocks',
        what: 'Each hour is built from a clock: music blocks with rotation and separation rules, and the US statutory rule for stations that need it. When the desk goes on air playout steps aside; when the show ends, playout holds the air.',
        href: '/docs/reference/playout/#the-schedule',
        link: 'Playout',
        home: 'Each hour is built from a clock with rotation and separation rules; when the desk goes on air playout steps aside, and when the show ends it holds the air.',
        shot: 'week',
      },
      {
        id: 'week',
        name: 'The week on a grid',
        what: 'Live shows, automation blocks, series and seasons on one week, with the episode, series, show and log editors beside it; a conflict is marked where it is, and a draft can be made from a rule.',
        href: '/docs/how-to/schedule-the-week/',
        link: 'Schedule the week',
        shot: 'week',
      },
      {
        id: 'library',
        name: 'A library and a per-play log',
        what: 'Files are imported with their tags, every play is logged with its time, and listener hours and sessions are kept, for the reports a licence asks for. A legal logger records the programme for as long as retention says.',
        href: '/docs/reference/playout/#what-the-station-keeps',
        link: 'What the station keeps',
      },
      {
        id: 'loudness',
        name: 'One loudness for the station',
        what: 'Playout\'s chain holds the stream near −16 LUFS with a peak limiter after it, so a live show and the music block after it sit at the same level in a listener\'s ears.',
        href: '/docs/reference/playout/#the-processing-chain',
        link: 'The processing chain',
      },
      {
        id: 'now-playing',
        name: 'Now playing, by the API',
        what: 'The show and the track on air, for the station\'s own website and the slate, from one route; up next is kept off for US statutory webcasters.',
        href: '/docs/reference/playout/#now-playing',
        link: 'Now playing',
      },
    ],
  },
  {
    id: 'listeners',
    title: 'Listeners',
    lead: 'A stream any player opens, video when a station wants it, and a page with the chat on it.',
    features: [
      {
        id: 'stream',
        name: 'An Ogg Opus stream, and MP3 when you need it',
        what: 'An Icecast-compatible stream in Opus at a rate the station sets, which every current player opens, and MP3 at two rates, 128 kbit/s for speech and 320 for music, for older players and the directories that need one, each off until a station turns it on. The listener chooses on the player page.',
        href: '/docs/reference/playout/#the-mp3-streams',
        link: 'The MP3 streams',
        home: 'An Icecast-compatible Ogg Opus stream, an optional MP3 stream, HLS with video, and a player page with the chat on it.',
        shot: 'player',
      },
      {
        id: 'video',
        name: 'Video, when a station turns it on',
        what: 'Cameras, a vision mixer and a slate with now playing and a still per show, captions, and AV1 in HLS behind the delay, with an audio-only toggle for the viewer. A guest\'s browser camera has gone through a station to an RTMP server; not yet through a whole show.',
        href: '/docs/reference/desk-video/',
        link: 'Video in the desk',
      },
      {
        id: 'player-page',
        name: 'A player page with call-in on it',
        what: 'The station\'s own page: the ON AIR lamp, the show on now, Listen live and Call the studio, served by the station with its caching and security headers set; a station\'s own website can use the open routes instead.',
        href: '/docs/reference/public-pages/',
        link: 'The public pages',
        shot: 'player',
      },
      {
        id: 'chat',
        name: 'A chat, moderated from the screener',
        what: 'Listeners chat on the player page, each with an avatar: one of the 72 that come with the station, or a picture of their own that staff or a moderator approve first. The screener\'s lanes show what needs them, every removal carries a statement of reasons, the minimum age follows the station\'s jurisdiction, and the chat keeps records only as long as retention says.',
        href: '/docs/how-to/moderate-the-chat/',
        link: 'Moderate the chat',
        shot: 'chat',
      },
      {
        id: 'two-latencies',
        name: 'Two latencies, on purpose',
        what: 'The people talking hear each other at once; listeners are seconds behind, which lets a player start at once, ride out a poor network and gives the presenter a delay to dump into.',
        href: '/docs/explanation/architecture/#two-latencies',
        link: 'Two latencies',
      },
    ],
  },
  {
    id: 'shows',
    title: 'Shows, podcasts and show cards',
    lead: 'Each show has a card, a feed and episodes that make themselves.',
    features: [
      {
        id: 'show-cards',
        name: 'Show cards drawn by the station',
        what: 'A Poster, a Slate and a square for every show, series and episode, drawn from the show\'s data and its still, so a link preview, a post and the podcast art match without a designer. Text is fitted, never cropped.',
        href: '/docs/reference/surfaces/cards/',
        link: 'Show cards',
        shot: 'poster',
      },
      {
        id: 'seasons',
        name: 'Seasons that fill in their own episodes',
        what: 'A series with a rule makes its episodes as the weeks come, live or pre-recorded; a correction to an episode that has aired is logged, not silent.',
        href: '/docs/reference/surfaces/cards/#seasons-that-fill-in-their-own-episodes',
        link: 'Seasons',
      },
      {
        id: 'podcast',
        name: 'A podcast feed per show',
        what: 'RSS with an Opus alternate for each show, from the episodes the schedule already knows; episode files are attached through the API.',
        href: '/docs/reference/surfaces/cards/#the-podcast-feed',
        link: 'The podcast feed',
      },
    ],
  },
  {
    id: 'running',
    title: 'Running it',
    lead: 'One installation on a server of your own, with the ways out to a bigger audience and other software.',
    features: [
      {
        id: 'one-installation',
        name: 'One installation, three routes',
        what: 'The room, the desk, playout, the stream, the chat and the pages in one bundle with one login: a script on a Linux server, a Cloudron package, or Quadlets under systemd. No stack of tools, each with its own idea of who hears whom.',
        href: '/run/',
        link: 'Run your own station',
        home: 'The room, the desk, playout, the stream, the chat and the pages in one bundle, by a script, a Cloudron package or Quadlets; not a stack of tools with five logins.',
      },
      {
        id: 'ports',
        name: 'Every port a setting',
        what: 'Each port the station opens is a setting with a free-port check, so it fits beside what the server already runs.',
        href: '/docs/reference/ports/',
        link: 'Ports',
      },
      {
        id: 'edges',
        name: 'Edges and a CDN when the audience grows',
        what: 'Content steering sends players to health-checked edges; an HLS edge and an audio relay image are shipped, and Bunny can front the station. Measured on local edges so far.',
        href: '/docs/reference/steering/',
        link: 'Content steering',
      },
      {
        id: 'api',
        name: 'An API with tokens, open routes and webhooks',
        what: 'Scoped tokens for other software, open read routes for the station\'s own website, signed webhook deliveries when something happens, and an OpenAPI description generated from the code.',
        href: '/docs/reference/integrations/',
        link: 'Integrations',
        home: 'Scoped tokens, open read routes for the station\'s own website, signed webhooks, and an OpenAPI description generated from the code.',
      },
      {
        id: 'retention',
        name: 'Retention by jurisdiction',
        what: 'Choose the station\'s jurisdiction and recordings, logs, chat and callers\' consent are kept as long as its rules ask and no longer; a hold keeps what a complaint concerns, and a sweeper removes the rest.',
        href: '/docs/how-to/set-retention/',
        link: 'Set how long the station keeps records',
        shot: 'retention',
      },
      {
        id: 'staff',
        name: 'Staff accounts and roles',
        what: 'Each person signs in with a password or an emailed code, a role says what they may do, and someone who leaves is removed in one place.',
        href: '/docs/how-to/add-staff/',
        link: 'Add staff and sign in',
      },
      {
        id: 'free-software',
        name: 'Free software, self-hosted',
        what: 'The code is AGPL-3.0-only and the documentation CC BY-SA 4.0. There is no hosted service and no account with anyone: the station is yours.',
        href: '/docs/reference/licence/',
        link: 'Licences',
      },
    ],
  },
  {
    id: 'studio',
    title: 'The studio app',
    lead: 'A desktop program for whoever runs the show, with every action on a key.',
    features: [
      {
        id: 'desk',
        name: 'The desk, the screener, the carts and the schedule',
        what: 'Six sources, the lamp, the delay, the audience and the picture on one screen; the screener\'s queue and the chat\'s lanes on the next tab; the cart wall and the week on the others. Light and dark, with a text size that is remembered.',
        href: '/docs/reference/surfaces/desk/',
        link: 'Desk',
        shot: 'desk',
      },
      {
        id: 'keymap',
        name: 'Every action on a key',
        what: 'One keymap across the tabs: the digits for the sources, Del Del to dump, F1 to F6 for the carts, F7 for the picture. A key whose feature is not in 1.0.0 says so in a note and does nothing to the room.',
        href: '/docs/reference/keymap/',
        link: 'Keymap',
      },
      {
        id: 'carts',
        name: 'A cart wall',
        what: 'Banks of jingles, beds and stings under the left hand, fired from any tab; a bed on air is ducked under an open microphone. A demo pack of 50 openly licensed sounds is in the repository.',
        href: '/docs/reference/surfaces/carts/',
        link: 'Cart wall',
        shot: 'carts',
      },
      {
        id: 'packages',
        name: 'Linux and Windows, with an updater',
        what: 'An AppImage, a .deb and an .rpm for Linux, and an unsigned installer for Windows; updates are signed, with a rollback for the AppImage that is built but not yet run on a real install. The server runs on Linux.',
        href: '/docs/reference/packages/',
        link: 'Packages and install files',
      },
    ],
  },
];

// The home page's list and the top of /features/: the strongest differences, in this order.
export const lead = ['one-installation', 'mix-minus', 'call-in', 'delay-and-dump', 'restarts', 'clocks', 'stream', 'api'];
export const leadFeatures = (): Feature[] => {
  const all = groups.flatMap((g) => g.features);
  return lead.map((id) => {
    const f = all.find((x) => x.id === id && x.home);
    if (!f) throw new Error(`features: no home line for ${id}`);
    return f;
  });
};

// Planned, and not in 1.0.0. Each says what works today instead; the status page is the list.
export interface Planned {
  id: string;
  name: string;
  what: string;
  today: string;
  href: string;
  link: string;
}

export const next: Planned[] = [
  {
    id: 'dj-handover',
    name: 'A handover window from the schedule',
    what: 'The desk opens the handover by itself before the post, from the schedule, with the incoming DJ cued and ready (ADR-0231).',
    today: 'In 1.0.1 the next DJ joins off air, and one key, ⇧⏎, puts them on air beside the presenter and then takes the presenter off; the presenter starts it when they are ready.',
    href: '/docs/reference/keymap/#desk-tab',
    link: 'The desk\'s keys',
  },
  {
    id: 'whip',
    name: 'WHIP for remote DJs',
    what: 'A DJ sends a live stream into the room from an encoder such as OBS, the standard way.',
    today: 'In 1.0.0 a remote DJ joins from a browser with a microphone.',
    href: '/docs/reference/glossary/',
    link: 'Glossary',
  },
  {
    id: 'presenter-kit',
    name: 'The presenter kit',
    what: 'For a presenter at home: one clock, radio presets, a patchbay, and VBAN in and out, in place of a virtual mixer on their machine.',
    today: 'Moved to release two (ADR-0207). In 1.0.0 presenters join from a browser and the studio app drives the desk.',
    href: '/docs/reference/surfaces/kit/',
    link: 'Presenter kit',
  },
  {
    id: 'telephone',
    name: 'A telephone line',
    what: 'Callers who dial a number.',
    today: 'Not in release one: callers press Call the studio on the station\'s page and talk through a browser. The comparison names the phone systems that take calls from a number.',
    href: '/compare/',
    link: 'The comparison',
  },
  {
    id: 'macos',
    name: 'A macOS studio app',
    what: 'The desk and the screener on a Mac.',
    today: 'A presenter or guest on a Mac joins from the browser as anyone does; running the desk from a Mac is not possible yet.',
    href: '/docs/reference/packages/',
    link: 'Packages and install files',
  },
];
