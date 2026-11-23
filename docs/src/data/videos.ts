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
    duration: '6:54',
    madeOn: '1.3.0',
    description:
      'Install a station on Bazzite or another RPM-based Linux system, name it, put a sound on, and hear it in a second window the way a listener does. Made for someone who has never run a station.',
    alt: "The first frame of the video: the listener page of a station called Burnside Radio, with a Listen live button and a Call the studio panel, and the caption 'Your own radio station, playing a song.' Bazzite and WeaveSignal 1.3.0 are named in the corner.",
    poster: '/media/tutorials/bazzite-1.3.0/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/bazzite-1.3.0/first-station-v1.3.0.webm',
    mp4: '/media/tutorials/bazzite-1.3.0/first-station-v1.3.0.mp4',
    captions: '/media/tutorials/bazzite-1.3.0/first-station.vtt',
    transcript: '/media/tutorials/bazzite-1.3.0/transcript.txt',
    chapters: [
      { title: '1 · What you will have', start: 0 },
      { title: '2 · Install the station', start: 33.6 },
      { title: '3 · Name your station', start: 124.1 },
      { title: '4 · Put a sound on', start: 179.2 },
      { title: '5 · Play it to listeners', start: 258 },
      { title: '6 · Hear it in another window', start: 303.8 },
      { title: '7 · Optional: speak into it', start: 332.4 },
      { title: '8 · If it does not work', start: 355.2 },
    ],
    credits: tutorialCredit,
    creditsPage: '/media/tutorials/CREDITS.md',
  },
  {
    slug: 'studio-app-windows',
    kind: 'tutorial',
    title: 'The studio app on Windows',
    duration: '7:08',
    madeOn: '1.3.0',
    description:
      'Get the installer, check it, install the studio app on Windows, sign in to a station, put a song on, play it to listeners and hear it out of your speakers; then your microphone in Edge, and what to do if it does not work.',
    alt: "The first frame of the video: Windows search finding the WeaveSignal app, marked with an arrow, and the caption 'The studio app on Windows, and sound.' Windows and WeaveSignal 1.3.0 are named in the corner.",
    poster: '/media/tutorials/windows-1.3.0/poster.jpg',
    size: [1920, 1080],
    webm: '/media/tutorials/windows-1.3.0/windows-studio-v1.3.0.webm',
    mp4: '/media/tutorials/windows-1.3.0/windows-studio-v1.3.0.mp4',
    captions: '/media/tutorials/windows-1.3.0/windows-studio.vtt',
    transcript: '/media/tutorials/windows-1.3.0/transcript.txt',
    chapters: [
      { title: '1 · What you will have', start: 0 },
      { title: '2 · Get the installer', start: 50 },
      { title: '3 · Check and install', start: 101.3 },
      { title: '4 · Sign in', start: 164.1 },
      { title: '5 · Put a song on', start: 214.2 },
      { title: '6 · Play it to listeners', start: 253 },
      { title: '7 · Sound out of your speakers', start: 289.4 },
      { title: '8 · Your microphone in Edge', start: 321.6 },
      { title: '9 · If it does not work', start: 369.4 },
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
    lead: 'Two tutorials for someone who has never run a station: each takes you from nothing to a station playing a sound that you hear the way a listener does.',
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
