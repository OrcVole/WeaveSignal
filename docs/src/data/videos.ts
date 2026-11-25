// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Every video on the website, in one place (src/pages/videos.astro draws the page from it). A later
// video is one more entry here, and its files in public/media at publish time. The media is not in
// the repository: the page lists a video only when its video and poster files are in
// public/media at build time, and links the other files only when they are there too.

export type Kind = 'tutorial' | 'demo' | 'moment';

export interface Chapter {
  /** The chapter's title as the video shows it, with its number. */
  title: string;
  /** Where it starts, in seconds. */
  start: number;
}

export interface Video {
  /** The anchor on the page: /videos/#<slug>. */
  slug: string;
  kind: Kind;
  title: string;
  /** The length, as shown. */
  duration: string;
  /** The release the video was made on; the page says plainly that the product may have moved on. */
  madeOn?: string;
  /** What it shows, honestly, in a sentence or two. */
  description: string;
  /** The poster picture's alt text: what the frame shows. */
  alt: string;
  /** Paths under public/, with the leading slash. */
  poster: string;
  /** The poster's size in pixels, for the layout before it loads. */
  size: [number, number];
  webm: string;
  mp4?: string;
  captions: string;
  transcript?: string;
  howto?: string;
  chapters?: Chapter[];
  /** A credit that must stay with the video (a song, a music bed). */
  credits?: string;
  /** The page of credits for the whole group of files. */
  creditsPage?: string;
}

const tutorialCredit =
  'Song: "I Move On", from the open movie Sintel. © copyright Blender Foundation | durian.blender.org. Licensed CC BY 3.0 (creativecommons.org/licenses/by/3.0/).';

export const videos: Video[] = [
  {
    slug: 'first-station-linux',
    kind: 'tutorial',
    title: 'Your first station on Linux',
    duration: '7:02',
    madeOn: '1.3.0',
    description:
      'Install a station on Bazzite or another RPM-based Linux system, name it, put a sound on, and hear it in a second window the way a listener does. Made for someone who has never run a station.',
    alt: "The poster: the studio desk of a station called Burnside Radio with the ON AIR lamp lit and a presenter, Ishbel, on air, with the WeaveSignal icon, a radio mast in a rounded square, in the lower right.",
    poster: '/media/tutorials/bazzite-1.3.0/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/bazzite-1.3.0/first-station-v1.3.0.webm',
    mp4: '/media/tutorials/bazzite-1.3.0/first-station-v1.3.0.mp4',
    captions: '/media/tutorials/bazzite-1.3.0/first-station.vtt',
    transcript: '/media/tutorials/bazzite-1.3.0/transcript.txt',
    chapters: [
      { title: '1 · What you will have', start: 0 },
      { title: '2 · Install the station', start: 39.4 },
      { title: '3 · Name your station', start: 130.2 },
      { title: '4 · Put a sound on', start: 185.2 },
      { title: '5 · Play it to listeners', start: 264 },
      { title: '6 · Hear it in another window', start: 309.9 },
      { title: '7 · Optional: speak into it', start: 338.5 },
      { title: '8 · If it does not work', start: 361.3 },
    ],
    credits: tutorialCredit,
    creditsPage: '/media/tutorials/CREDITS.md',
  },
  {
    slug: 'studio-app-windows',
    kind: 'tutorial',
    title: 'The studio app on Windows',
    duration: '7:19',
    madeOn: '1.3.0',
    description:
      'Get the installer, check it, install the studio app on Windows, sign in to a station, put a song on, play it to listeners and hear it out of your speakers; then your microphone in the browser, and what to do if it does not work.',
    alt: "The poster: the studio desk of a station called Burnside Radio on standby, with nobody in the room yet, and the WeaveSignal icon, a radio mast in a rounded square, in the lower right.",
    poster: '/media/tutorials/windows-1.3.0/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/windows-1.3.0/windows-studio-v1.3.0.webm',
    mp4: '/media/tutorials/windows-1.3.0/windows-studio-v1.3.0.mp4',
    captions: '/media/tutorials/windows-1.3.0/windows-studio.vtt',
    transcript: '/media/tutorials/windows-1.3.0/transcript.txt',
    chapters: [
      { title: '1 · What you will have', start: 0 },
      { title: '2 · Get the installer', start: 55.4 },
      { title: '3 · Check and install', start: 109.1 },
      { title: '4 · Sign in', start: 171.6 },
      { title: '5 · Put a song on', start: 221.8 },
      { title: '6 · Play it to listeners', start: 260.5 },
      { title: '7 · Sound out of your speakers', start: 298.1 },
      { title: '8 · Your microphone in the browser', start: 332.6 },
      { title: '9 · If it does not work', start: 381.8 },
    ],
    credits: tutorialCredit,
    creditsPage: '/media/tutorials/CREDITS.md',
  },
  {
    slug: 'four-views',
    kind: 'tutorial',
    title: 'The four views: Solo, Guests, Callers, Full',
    duration: '3:57',
    madeOn: '1.3.1',
    description:
      'A show runs at one of four steps, and each adds one kind of person: Solo is just you, Guests lets you invite someone by link, Callers opens the phone lines with a delay and Dump, and Full adds a screener, the picture and the next DJ. Made with the studio app\'s browser build.',
    alt: "The poster: the desk at the Solo step, with the panel 'Just you on this show' saying that guests, callers and the screener's seat are switched off, a Move up a step button, and the caption 'Solo is just you.'",
    poster: '/media/tutorials/four-views-1.3.1/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/four-views-1.3.1/four-views-v1.3.1.webm',
    mp4: '/media/tutorials/four-views-1.3.1/four-views-v1.3.1.mp4',
    captions: '/media/tutorials/four-views-1.3.1/four-views.vtt',
    transcript: '/media/tutorials/four-views-1.3.1/transcript.txt',
    chapters: [
      { title: '1 · Four steps, one show', start: 0 },
      { title: '2 · Solo: just you', start: 43.9 },
      { title: '3 · Guests: you and a guest', start: 87.7 },
      { title: '4 · Callers: listeners ring in', start: 129.7 },
      { title: '5 · Full, and moving down', start: 172.1 },
    ],
    credits: tutorialCredit,
    creditsPage: '/media/tutorials/CREDITS.md',
  },
  {
    slug: 'a-solo-show',
    kind: 'tutorial',
    title: 'A solo show',
    duration: '2:12',
    madeOn: '1.3.1',
    description:
      'A whole show with just you: join with your microphone link, light the lamp, see a listener press Listen live and hear you about 24 seconds later, play carts, and end the show so that automation takes the air. Made with the studio app\'s browser build.',
    alt: "The poster: a presenter's strip on the desk, Ishbel on standby with her level meter, the On, Cue, Talk and Mute keys, and the caption 'A whole show, with just you.'",
    poster: '/media/tutorials/a-solo-show-1.3.1/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/a-solo-show-1.3.1/a-solo-show-v1.3.1.webm',
    mp4: '/media/tutorials/a-solo-show-1.3.1/a-solo-show-v1.3.1.mp4',
    captions: '/media/tutorials/a-solo-show-1.3.1/a-solo-show.vtt',
    transcript: '/media/tutorials/a-solo-show-1.3.1/transcript.txt',
    chapters: [
      { title: '1 · Get ready', start: 0 },
      { title: '2 · Light the lamp', start: 38.1 },
      { title: '3 · Sounds, and the end', start: 64.9 },
    ],
    credits: tutorialCredit,
    creditsPage: '/media/tutorials/CREDITS.md',
  },
  {
    slug: 'demo',
    kind: 'demo',
    title: 'The demo show',
    duration: '3:35',
    description:
      'A morning show on a made-up station, with sound: the cast introduce themselves, a guest joins from a link, a caller is screened and taken to air, a swear is dumped before listeners hear it, playout falls over and comes back, and automation takes the air when the show ends.',
    alt: "A frame from the demo show: the desk with the ON AIR lamp lit for The Morning Kettle on Kettle FM, the presenter and a guest on air and a caller, Niall Anderson, with the screener's line under his name.",
    poster: '/media/poster.jpg',
    size: [1920, 1080],
    webm: '/media/demo.webm',
    mp4: '/media/demo.mp4',
    captions: '/media/demo.vtt',
    credits: 'Music bed "Wholesome" by Kevin MacLeod (incompetech.com), CC BY 4.0; jingle by Kenney (CC0). Every voice is designed, not cloned from anyone.',
    creditsPage: '/media/CREDITS.txt',
  },
  {
    slug: 'moment-guest-joins',
    kind: 'moment',
    title: 'A guest joins from a link',
    duration: '0:26',
    description: 'A guest joins the show with nothing but the link in an email: no account, no app, one button.',
    alt: "The desk with the guest's invitation page beside it: 'Join The Morning Kettle as a guest', a Check your microphone button and a Join the studio button.",
    poster: '/media/moment-guest-joins.jpg',
    size: [1280, 720],
    webm: '/media/moment-guest-joins.webm',
    captions: '/media/moment-guest-joins.vtt',
  },
  {
    slug: 'moment-call-in',
    kind: 'moment',
    title: 'A listener calls in',
    duration: '0:23',
    description: "A listener calls from the station's own page on a phone: no number to dial, and consent to record comes first. The screener answers, asks who he is and whether he has rung before.",
    alt: "The desk with the station's player page on a phone beside it: the Call the studio form with a name, a topic and the consent to being recorded.",
    poster: '/media/moment-call-in.jpg',
    size: [1280, 720],
    webm: '/media/moment-call-in.webm',
    captions: '/media/moment-call-in.vtt',
  },
  {
    slug: 'moment-on-air',
    kind: 'moment',
    title: 'The show goes on air',
    duration: '0:17',
    description: 'One key puts the presenter on air, and the lamp says ON AIR only when listeners can hear.',
    alt: 'The desk with the presenter, Eilidh Fraser-MacLeod, on air: the ON AIR lamp is lit and the show is The Morning Kettle.',
    poster: '/media/moment-on-air.jpg',
    size: [1280, 720],
    webm: '/media/moment-on-air.webm',
    captions: '/media/moment-on-air.vtt',
  },
  {
    slug: 'moment-dump',
    kind: 'moment',
    title: 'A swear is dumped',
    duration: '0:26',
    description: 'A caller swears; the presenter dumps it. Listeners hear the call and then the presenter apologising, and never the swear, because the stream runs a few seconds behind the room.',
    alt: "The desk with a caller, Jessie Brown, on air, the profanity delay showing 8.0 s and a Dump key beside it; the caption reads her first words.",
    poster: '/media/moment-dump.jpg',
    size: [1280, 720],
    webm: '/media/moment-dump.webm',
    captions: '/media/moment-dump.vtt',
  },
  {
    slug: 'moment-playout-down',
    kind: 'moment',
    title: 'Playout goes down and comes back',
    duration: '0:17',
    description: 'The lamp says PLAYOUT DOWN: listeners cannot hear the show, but the people in the room still hear each other. Playout is restarted and the show carries on.',
    alt: 'The desk with the lamp saying PLAYOUT DOWN, the two people in the room marked OPEN, and a Restart playout button.',
    poster: '/media/moment-playout-down.jpg',
    size: [1280, 720],
    webm: '/media/moment-playout-down.webm',
    captions: '/media/moment-playout-down.vtt',
  },
];

export const groups: { id: string; title: string; lead: string; kinds: Kind[] }[] = [
  {
    id: 'start',
    title: 'Start here',
    lead: 'Tutorials for someone who has never run a station. The first two take you from nothing to a station playing a sound that you hear the way a listener does; the next two show the four steps a show can run at, and a whole show on your own.',
    kinds: ['tutorial'],
  },
  {
    id: 'see',
    title: 'See it work',
    lead: 'A made-up morning show on a made-up station, whole, and the moments from it one at a time.',
    kinds: ['demo', 'moment'],
  },
];

/** A time as m:ss, for a chapter's start. */
export const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
