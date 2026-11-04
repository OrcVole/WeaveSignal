// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The questions a stranger arrives with (ADR-0211): eight on the home page, all on /faq/. Each
// answer is short and links the page that says more; the answers are HTML strings so they can
// carry links, and every fact in them is on the page they link.
export interface Question {
  id: string;
  q: string;
  a: string[];
  home?: boolean;
}

export const questions: Question[] = [
  {
    id: 'free',
    home: true,
    q: 'Is it Free, and what does running it cost?',
    a: [
      'Yes, in both senses. It is Free as in Freedom: under the <a href="/docs/reference/licence/">GNU AGPL, version 3 only</a>, you may run, study, change and share it, and anyone who offers a changed version to others over a network must share its source too. It is also free as in beer: there is no licence fee and no paid edition.',
      'What you pay for is a server, a domain name and the time to run them.',
      'Support is best effort, through <a href="https://forgejo.weavesignal.top/WeaveSignal/weavesignal/issues">the issue tracker</a> and <a href="mailto:info@weavesignal.top">email</a>; there is no paid support.',
    ],
  },
  {
    id: 'hardware',
    home: true,
    q: 'What hardware does a station need?',
    a: [
      'Less than you might think, as long as it is not shared with something busy. With video off, the whole station peaked at about 700 MiB of memory under a test load, and the Cloudron package is limited to 1.5 GiB. It ran for 24 hours on a dedicated 4-vCPU server with no restart and no error, its memory flat; a 2-vCPU server with 2–4 GB of memory should do for an audio-only station.',
      'The one thing live audio needs is CPU on time: on a heavily shared server the listener stream ran dry even with a 3 s cushion. Give a station on air a server of its own. <a href="/docs/reference/resources/">Memory and CPU</a> has the measurements.',
    ],
  },
  {
    id: 'players',
    home: true,
    q: 'Can my existing players, Icecast relays and directory listings still work?',
    a: [
      'The listener stream is served Icecast-style, as Ogg Opus, with an optional MP3 stream for older players; so an ordinary player, a station website\'s embedded player, or a smart speaker that takes a stream address can use it.',
      'For relays, WeaveSignal ships its own Icecast 2.5.0 relay image. A third-party Icecast relay should be able to pull the mount as an ordinary listener, with its SHOUTcast-metadata relaying turned off; this has not been tested.',
      'The directory setting (<code>DIRECTORY_LISTING</code>) only marks the stream as public; it does not submit the station to any directory. A directory that takes a stream address by hand can be given the Opus address, or the MP3 one where that stream is on. <a href="/docs/reference/playout/#directory-listing">Playout</a> says more.',
    ],
  },
  {
    id: 'app',
    home: true,
    q: 'Do presenters need the app?',
    a: [
      'No. Presenters, guests and callers join from a browser; so do listeners. The studio app is for whoever runs the desk and screens the calls: it shows the desk, the screener, the cart wall and the schedule and sends their keys to the station. It carries no audio itself.',
    ],
  },
  {
    id: 'macos',
    home: true,
    q: 'Does it run on macOS?',
    a: [
      'There is no macOS build of the studio app yet. A presenter or guest on a Mac joins from the browser as anyone does; running the desk from a Mac is not possible yet. The server runs on Linux.',
    ],
  },
  {
    id: 'packaging',
    q: 'How is it packaged?',
    a: [
      'The server comes three ways: a Compose bundle installed by <code>weavesignal.sh</code> on any Linux server with Docker Compose or Podman; a Cloudron app, installed from its published image with Cloudron\'s command-line tool; and Quadlets for systemd, generated from the same bundle. All three run the same published images.',
      'The studio app comes as an AppImage, a <code>.deb</code> and an <code>.rpm</code> for Linux (64-bit Intel and AMD), and as an unsigned Windows installer (<code>.exe</code>) that has been built but not yet run on Windows. <a href="/run/">Run your own station</a> has the commands and the downloads; <a href="/docs/reference/packages/">Packages and install files</a> lists every file.',
    ],
  },
  {
    id: 'chat',
    home: true,
    q: 'What if I already have chat software, say Rocket.Chat?',
    a: [
      'The built-in chat sits on the station\'s player page, and that is what it gives you: listeners read without an account and post with a light one, signed in by an emailed code; the screener moderates it from the same tab as the callers, with a statement of reasons for each action; and what is kept, for how long, and the minimum age follow the retention preset you chose for your country.',
      'An existing chat is not bridged: its accounts, moderation and retention stay its own. There is no setting to turn the built-in chat off yet, so a station with a Rocket.Chat community can leave the built-in one unused and link its own from its website. A setting to use an external chat instead is being considered.',
    ],
  },
  {
    id: 'azuracast',
    home: true,
    q: 'How is it different from AzuraCast?',
    a: [
      'AzuraCast is a mature web admin for playout and streaming, hosting several stations on one installation, with years of community knowledge. WeaveSignal is the live studio: a room where presenters, guests and callers hear each other through a desk, a screener for the callers, a chat moderated from the same place, and playout between shows; one station per installation, and new.',
      'If you mainly play music or recorded shows, AzuraCast with a guest link such as Cleanfeed may serve you better. <a href="/compare/">The comparison</a> says where each is stronger.',
    ],
  },
  {
    id: 'ready',
    home: true,
    q: 'Is it ready for a real station?',
    a: [
      'Release 1.0.0 is the first public release. A station has been installed from the published images on a public server, and a real caller has rung in from a phone and gone to air. Some parts have not run end to end yet: the status page lists them. It has streamed for 24 hours on a server of its own with no restart; two people talking over two real networks has been measured but not judged by ear, and the Windows installer has not been run on Windows.',
      'So: run a test show before you put a real one on air, on a server of its own, and read <a href="/docs/reference/release-one-status/">what 1.0.0 does and leaves out</a>.',
    ],
  },
  {
    id: 'telephone',
    q: 'Can callers phone in from a telephone?',
    a: [
      'Not in release one. Callers press Call the studio on the station\'s page and talk through their phone\'s or computer\'s browser; there is no telephone number, no SIP line and no phone hybrid. If callers must dial a number, see the <a href="/compare/">comparison</a> for the phone systems that do that.',
    ],
  },
  {
    id: 'hosted',
    q: 'Is there a hosted version?',
    a: [
      'No. WeaveSignal is self-hosted only: you run it on your own server, with the one-line script, Cloudron or Quadlets. If you do not want to run a server, a hosted service such as Radio.co or Live365 is the other kind of answer; the <a href="/compare/">comparison</a> lists them.',
    ],
  },
  {
    id: 'help',
    q: 'Who makes it, and where do I get help?',
    a: [
      'The WeaveSignal contributors. The code and the issue tracker are on <a href="https://forgejo.weavesignal.top/WeaveSignal/weavesignal">the project\'s Forgejo</a>, where anyone can sign up and open an issue, with a <a href="https://github.com/OrcVole/WeaveSignal">mirror on GitHub</a>. Anything else: <a href="mailto:info@weavesignal.top">info@weavesignal.top</a>. A security problem goes by email with "Security" in the subject, not in a public issue.',
    ],
  },
  {
    id: 'privacy',
    q: 'Does this website track me?',
    a: [
      'It counts visits with Umami, which sets no cookies and keeps nothing that identifies you: which pages are read, where visitors came from, and their country, browser and kind of device, as totals. There is nothing to consent to and no banner. The figures are the project\'s own, on a server it runs, and are never shared.',
    ],
  },
  {
    id: 'versioning',
    q: 'What does the version number promise?',
    a: [
      'Semantic versioning. 1.0.0 is the first release whose public API, settings and Compose bundle we promise to keep compatible: within 1.x, an update does not break a station, an integration or a settings file. A change that would is a 2.0.0. The number is 1.0.0 rather than 0.1.0 because that promise starts now, not because every part is proven: <a href="/docs/reference/release-one-status/">the status page</a> says which parts are.',
    ],
  },
];
