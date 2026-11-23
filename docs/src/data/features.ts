// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// What WeaveSignal can do, grouped by what a station cares about, for /features/ and the home
// page's short list (`lead`, the entries marked `home`). Every feature here is in 1.3.0:
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
        what: 'Each presenter\'s and DJ\'s voice goes through a gate, a compressor, a leveller and a limiter set in the station\'s settings, and every mix has a limiter of its own, so a loud guest cannot clip the programme. The gate holds open for 250 ms, so the soft end of a word is kept.',
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
      {
        id: 'own-studios',
        name: 'Hosts in their own studios',
        what: 'A host sends their show from their own software (BUTT, Mixxx, OBS, vMix, ffmpeg) over Icecast or SRT, with a key for their show. Their source stands by on the desk, levelled, until it is put on air; outside the show\'s time a connection is a private test.',
        href: '/docs/how-to/set-up-a-host-in-their-own-studio/',
        link: 'Set up a host in their own studio',
      },
      {
        id: 'pull',
        name: 'A partner\'s stream, pulled',
        what: 'The station opens a partner station\'s Icecast mount, an HTTP or HTTPS stream, an HLS playlist or an SRT listener itself, from before the show\'s time, and it stands by on the desk like a host\'s own studio. Addresses inside the station\'s own network are refused unless allowed. Made through the API.',
        href: '/docs/how-to/pull-a-stream/',
        link: 'Pull a partner\'s stream into a show',
      },
      {
        id: 'whip',
        name: 'OBS over WHIP',
        what: 'A host pastes an address and their show key\'s secret into OBS\'s WHIP output, and their sound reaches the desk over WebRTC on one UDP port the station answers itself, with no STUN or TURN. Sound only: OBS\'s picture is taken and set aside. Tried with GStreamer\'s WHIP client; a real OBS has not been tried yet.',
        href: '/docs/how-to/send-from-obs-over-whip/',
        link: 'Send from OBS over WHIP',
      },
      {
        id: 'bridge',
        name: 'The studio app as a bridge',
        what: 'A host behind a router sends from their own software to the studio app on their own computer, and the app carries it to the station over one connection on the station\'s HTTPS port: no port to open at either end.',
        href: '/docs/how-to/send-through-the-bridge/',
        link: 'Send through the bridge',
      },
      {
        id: 'vban',
        name: 'A studio PC over VBAN, and one clock',
        what: 'On Compose and the Quadlets, a studio PC on the station\'s own network sends VBAN as a show\'s source, the desk sends VBAN back (the programme before or after the delay, or a monitor mix) set by presets, and every source follows the desk\'s clock instead of skipping as it drifts. The presenter kit\'s station half.',
        href: '/docs/how-to/connect-a-studio-pc-over-vban/',
        link: 'Connect a studio PC over VBAN',
      },
      {
        id: 'monitor-link',
        name: 'A monitor link for each host',
        what: 'A page the host keeps open beside their software: a lamp that says whether listeners hear them, counts on the station\'s clock, the station without their own voice in their headphones, and the few buttons they need, including Go on air and Dump.',
        href: '/docs/how-to/use-your-monitor-link/',
        link: 'Use your monitor link',
      },
      {
        id: 'handover-window',
        name: 'The rota and the handover window',
        what: 'The coming hours\' shows with who is down for each, from the room or their own studio, and whether they are ready. Before each show the desk opens a window, five minutes by default, that names the next host and gives them one heads-up on the desk and their monitor link; it changes nothing on air.',
        href: '/docs/how-to/follow-the-rota/',
        link: 'Follow the rota',
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
      {
        id: 'telephone',
        name: 'A telephone line',
        what: 'On Compose and the Quadlets, listeners can ring a number on the station\'s own SIP trunk: they hear a few words, press a key, and wait in the screener\'s queue like any caller. The station keeps a ban key and the last four digits, never the number. Off until a trunk is set, and not yet tried through a provider\'s trunk.',
        href: '/docs/how-to/take-phone-calls/',
        link: 'Take phone calls',
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
        what: 'The listener process holds the stream and keeps a cushion for every player. Playout and the desk can stop or restart underneath it (the desk restarts playout on Ctrl+R pressed twice): players hear the fallback for a few seconds, then the programme again, and are never disconnected.',
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
      {
        id: 'post',
        name: 'The schedule hands over at the post',
        what: 'When a show\'s time comes and its host is standing by, they go on air to the second; a late host is counted in; a DJ already on air keeps the air for a grace, then the schedule hands over. A host whose link drops is covered by automation, their place kept, and they go back on air when the link is steady.',
        href: '/docs/how-to/set-up-a-host-in-their-own-studio/#what-the-station-does-at-the-shows-post',
        link: 'What the station does at the post',
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
        id: 'new-show',
        name: 'New show, guided like calendar software',
        what: 'Once, every week, every month on the third Tuesday or the last Friday, every year on a date, or on dates you choose, with an end. The station says it back in a sentence, shows the dates, marks the clocks changing and what clashes, offers a clear time, and drafts the first episode. Dates can be skipped or moved, and a changed rule moves the episodes already drafted.',
        href: '/docs/reference/show-rules/',
        link: 'When a show airs: the rule',
      },
      {
        id: 'calendar-files',
        name: 'Calendar files in and out',
        what: 'The schedule, or one show, as a calendar link a listener subscribes to, with each show\'s repeat, skipped and moved dates. A calendar file from another system comes in as new shows, each said back with its dates and clashes before anything is made, with a pasted path or a dropped file.',
        href: '/docs/reference/calendar-files/',
        link: 'Calendar files: import and export',
      },
      {
        id: 'clashes',
        name: 'Clashing shows folded, a Day view and each show\'s dates',
        what: 'Shows that share a time fold in the week, one open and the others as tabs you press to bring forward; a Day view gives each clashing show a lane. A list of shows opens each at its dates: skip, move, draft or put back one, with Undo. The keys D and W switch between day and week.',
        href: '/docs/how-to/schedule-the-week/',
        link: 'Schedule the week',
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
        id: 'desk-gpu',
        name: 'Video on a graphics card',
        what: 'On Compose and the Quadlets the desk encodes the picture on an NVIDIA card with NVENC when the server has one, found by the install script on a new install or named in a setting, and on the CPU with SVT-AV1 otherwise. An update never moves a station to the card: Settings says when one is there and not used. A Cloudron app has no GPU. Not yet measured on a card with this release.',
        href: '/docs/how-to/use-a-gpu-for-video/',
        link: 'Use a GPU for video',
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
        what: 'The room, the desk, playout, the stream, the chat and the pages in one bundle with one login: a script on a Linux server, a Cloudron package, or Quadlets under systemd. No stack of tools, each with its own idea of who hears whom. On Compose and Cloudron the show, its rundown and the banned list are settings, and a station not set up yet says so on its own address.',
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
        what: 'Scoped tokens for other software, open read routes for the station\'s own website (on a Cloudron station too), signed webhook deliveries when something happens, and an OpenAPI description generated from the code.',
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
        what: 'Each person signs in with a password or an emailed code, a role says what they may do, and someone who leaves is removed in one place. A show can be tied to its presenter\'s account.',
        href: '/docs/how-to/add-staff/',
        link: 'Add staff and sign in',
      },
      {
        id: 'no-stun',
        name: 'No outside STUN server',
        what: 'The station, its desk and its join pages ask no outside server for anyone\'s public address unless the station names one: guests reach it directly or through its own relay. A station that wants one chooses from a short list of free-software projects\' servers or types its own.',
        href: '/docs/how-to/choose-a-stun-server/',
        link: 'Choose a STUN server',
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
        what: 'One keymap across the tabs: the digits for the sources, Del Del to dump, F1 to F6 for the carts, F7 for the picture. A key whose feature is not built says so in a note and does nothing to the room.',
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
        id: 'stations',
        name: 'Several stations in one studio app',
        what: 'The station\'s name in the frame lists the stations this computer keeps, each with its own address, sign-in and key, and nothing shared; Ctrl+Shift+S opens the list. Leaving a station that is on air is asked first, and nothing of one station shows on another. The window opens maximised on a small screen.',
        href: '/docs/how-to/work-with-several-stations/',
        link: 'Work with several stations',
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

// Planned, and not in 1.2. Each says what works today instead; the status page is the list.
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
    id: 'host-picture',
    name: 'A host\'s own picture, passed through',
    what: 'A host who encodes their picture on their own GPU sends it with their sound, and the desk passes it to viewers without encoding it again where the codecs allow (ADR-0289).',
    today: 'In 1.2 the station takes only a host\'s sound; viewers see the desk\'s own picture, encoded on the station\'s card (Compose and the Quadlets) or its CPU.',
    href: '/docs/reference/desk-video/#a-hosts-own-picture',
    link: 'A host\'s own picture',
  },
  {
    id: 'presenter-kit',
    name: 'The presenter kit on the presenter\'s computer',
    what: 'For a presenter at home: the microphone\'s chain, WeaveSignal as a microphone in other apps, headphones and a safety recording, in place of a virtual mixer on their machine.',
    today: 'From 1.2 the station\'s half is built on Compose and the Quadlets: VBAN in and out at the desk, routing presets and one clock. A presenter at home joins from a browser or sends from their own software.',
    href: '/docs/reference/surfaces/kit/',
    link: 'Presenter kit',
  },
  {
    id: 'telephone-cloudron',
    name: 'The telephone line on Cloudron',
    what: 'Callers who dial a number, on a Cloudron station too.',
    today: 'From 1.2 the telephone line runs on Compose and the Quadlets, behind the station\'s own SIP trunk; a Cloudron station takes callers from its page.',
    href: '/docs/how-to/take-phone-calls/',
    link: 'Take phone calls',
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
