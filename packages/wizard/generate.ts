// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
//
// The install wizard's answers turned into what a new station needs (ADR-0237): the settings
// file for the chosen route, the DNS records, the ports to open, the commands in order, and an
// answers file for the station's own setup page. Pure functions: nothing here reads the
// network, the clock or the browser, so the website's page can promise that it sends nothing.
//
// The three routes take their settings differently, and this file follows the scripts:
//
//   compose, quadlets  deploy/compose/weavesignal.sh reads `.env` line by line (`KEY=value`,
//                      the value as written), keeps what is there, makes the secrets, and
//                      writes the file whole. Compose then substitutes the values into every
//                      service's settings. So a value may hold no quote, backslash, dollar
//                      sign, backtick, hash or line break. The consent words are the one
//                      exception: the script keeps them in single quotes (each apostrophe as
//                      \'), and refuses only a backslash, a dollar sign and a per cent sign.
//   cloudron           deploy/cloudron/start.sh sources `/app/data/weavesignal.env` as shell,
//                      before it reads Cloudron's own variables. So the file is complete (the
//                      script's first file names settings it never adds later), every value is
//                      quoted, and the domains and public ports, which are Cloudron's install
//                      dialog's, are never in it. The consent words go in double quotes with
//                      every character the shell would act on escaped.
//
// No secret is asked, made or shown: both routes make them on the server.
import { normaliseHex } from './accent.ts';
import { PRESETS } from './presets.ts';

export type Route = 'compose' | 'quadlets' | 'cloudron';

export interface Answers {
  route: Route;
  /** The station's name, as listeners read it. */
  name: string;
  /** How a caller asks for their call to be taken down: an email address or a link (ADR-0124). */
  contact: string;
  /** Compose and Quadlets: reachable from the internet, or this machine only. */
  isPublic: boolean;
  /** Compose and Quadlets, public: the station's one name. */
  domain: string;
  /** Cloudron: the four names. */
  studioDomain: string;
  publicDomain: string;
  listenDomain: string;
  roomDomain: string;
  /** Whether the server has an IPv6 address (it decides the DNS list, ADR-0217). */
  ipv6: boolean;
  /** Compose and Quadlets, public: where the certificate authority writes about expiry. */
  acmeEmail: string;
  /** LiveKit's public IPv4 address, when discovery picks the wrong one; usually empty. */
  nodeIp: string;
  /** The retention preset's key (presets.ts). */
  jurisdiction: string;
  /** Cloudron: ask listeners for a password. */
  streamPassword: boolean;
  video: boolean;
  /** Compose and Quadlets, with video: a GPU for the desk (17e). `auto` leaves it to weavesignal.sh,
   *  which looks for an NVIDIA CDI spec; Cloudron has none. */
  gpu: GpuChoice;
  mail: 'off' | 'outbox' | 'cloudron';
  /** Compose and Quadlets: the sender address of sign-in codes. */
  mailFrom: string;
  searchListing: boolean;
  directoryListing: boolean;
  mediaPort: number;
  turnTlsPort: number;
  /** 0 is off. */
  turnUdpPort: number;
  /** 0 is off. */
  iceTcpPort: number;
  /** Hosts in their own studios (1.1, ADR-0280): Icecast sources, TCP; 0 is off. */
  sourceIcecastPort: number;
  /** Hosts in their own studios: SRT sources, UDP; 0 is off. */
  sourceSrtPort: number;
  /** The accent colour, `#rrggbb`, or empty for the product's own. */
  accent: string;
  /** Outside STUN servers (ADR-0290): none unless the station names one ([`STUN_CHOICES`]). */
  stun: StunChoice;
  /** For `other`: `host:port` each, split by commas or spaces, at most four. */
  stunOther: string;
  /** The station's own consent words (issue #26), on one line; empty, the shipped words. */
  consentText: string;
}

/** A GPU for the desk's video (17e): looked for by the script, an NVIDIA card by its CDI name, or none. */
export type GpuChoice = 'auto' | 'nvidia' | 'none';
export const GPU_CHOICES: readonly { key: GpuChoice; label: string }[] = [
  { key: 'auto', label: 'Look for one when the script runs (recommended)' },
  { key: 'nvidia', label: 'An NVIDIA card: encode on it with NVENC' },
  { key: 'none', label: 'None: encode on the CPU with SVT-AV1' },
];

/** The STUN chooser's answers. */
export type StunChoice = 'none' | 'nextcloud' | 'freeswitch' | 'other';

/** What the chooser offers (ADR-0290): none first, then the free-software projects' servers the
 *  packages reference lists (docs: reference/packages.md, *STUN servers*), then a typed list.
 *  Google's and Twilio's are left out on purpose. */
export const STUN_CHOICES: readonly { key: StunChoice; label: string; servers: readonly string[]; runBy?: string }[] = [
  { key: 'none', label: 'None (recommended): the station contacts no outside STUN server', servers: [] },
  { key: 'nextcloud', label: 'Nextcloud’s: stun.nextcloud.com:443', servers: ['stun.nextcloud.com:443'], runBy: 'Nextcloud GmbH, for Nextcloud Talk' },
  { key: 'freeswitch', label: 'FreeSWITCH’s: stun.freeswitch.org:3478', servers: ['stun.freeswitch.org:3478'], runBy: 'the FreeSWITCH project' },
  { key: 'other', label: 'Another server…', servers: [] },
];
/** At most this many (weavesignal-proto's `MAX_STUN_SERVERS`). */
export const MAX_STUN_SERVERS = 4;

/** The consent words a station has until it writes its own (the control plane's `CONSENT_TEXT`). */
export const DEFAULT_CONSENT_TEXT =
  "You'll be live on {station}, and we may keep your call in our archive and podcast {kept}. To have it taken down, contact {contact}.";
/** The fill-ins the consent words understand (the control plane's `CONSENT_PLACEHOLDERS`), and
 *  what the station puts in each. */
export const CONSENT_FILL_INS: readonly { token: string; means: string }[] = [
  { token: '{station}', means: 'the station’s name' },
  { token: '{kept}', means: 'how long recordings are kept, from the jurisdiction you chose, such as “for 42 days”' },
  { token: '{contact}', means: 'the takedown contact: the show’s own if it has one, then the station’s' },
  { token: '{presenter}', means: 'the presenter of the show on air; with no show on air, or a show without a presenter, the station’s name' },
];

/** An example of a station's own words that names the show's presenter, shown under the field
 *  (1.2.1): it uses all four fill-ins and nothing an install route refuses. */
export const CONSENT_EXAMPLE = "You'll be live with {presenter} on {station}, and we may keep your call {kept}. To have it taken down, contact {contact}.";

/** What a new station is offered before anyone answers. */
export function defaults(route: Route = 'compose'): Answers {
  return {
    route,
    name: '',
    contact: '',
    isPublic: true,
    domain: '',
    studioDomain: '',
    publicDomain: '',
    listenDomain: '',
    roomDomain: '',
    ipv6: false,
    acmeEmail: '',
    nodeIp: '',
    jurisdiction: 'none',
    streamPassword: false,
    video: false,
    gpu: 'auto',
    mail: route === 'cloudron' ? 'cloudron' : 'off',
    mailFrom: '',
    searchListing: false,
    directoryListing: false,
    mediaPort: 7882,
    turnTlsPort: route === 'cloudron' ? 15349 : 5349,
    turnUdpPort: 0,
    iceTcpPort: 0,
    sourceIcecastPort: 0,
    sourceSrtPort: 0,
    accent: '',
    stun: 'none',
    stunOther: '',
    consentText: '',
  };
}

/** The demo station's name: a station still called this has not been set up. */
export const DEMO_NAME = 'Kettle FM';
export const NAME_MAX = 200;
export const CONTACT_MAX = 200;

export type Field = keyof Answers;
export interface Problem {
  field: Field;
  message: string;
}

/** Characters a settings file would mis-read (the header says why). */
const FILE_UNSAFE = /["\\$`#]/;
const CONTROL = /[\u0000-\u001f\u007f]/;
const DOMAIN = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;

/** The name's problem for a settings file, or `null`. The setup page stores names a file cannot hold. */
export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Give the station a name.';
  if ([...n].length > NAME_MAX) return `A name is at most ${NAME_MAX} characters.`;
  if (CONTROL.test(n)) return 'A name is one line.';
  // Compose reads a value that starts with a quote as a quoted one.
  if (/^['"]/.test(n)) return 'A settings file cannot hold a name that starts with a quote mark.';
  if (FILE_UNSAFE.test(n)) return 'A settings file cannot hold " \\ $ ` or # in the name. Leave them out here; the station\'s setup page can add them later.';
  return null;
}

/** The contact's problem for a settings file, or `null`. Empty is allowed: call-in stays closed. */
export function contactProblem(contact: string): string | null {
  const c = contact.trim();
  if (!c) return null;
  if ([...c].length > CONTACT_MAX) return `A contact is at most ${CONTACT_MAX} characters.`;
  if (/\s/.test(c) || CONTROL.test(c)) return 'A contact is one email address or one link, without spaces.';
  if (FILE_UNSAFE.test(c)) return 'A settings file cannot hold " \\ $ ` or # in the contact. Use an address or link without them.';
  if (!EMAIL.test(c) && !/^https?:\/\/[^\s/]+\.[^\s/]+/.test(c)) return 'Give an email address (calls@station.example) or a link that starts with https://.';
  return null;
}

function domainProblem(value: string, what: string): string | null {
  const d = value.trim();
  if (!d) return `Give ${what}.`;
  if (!DOMAIN.test(d)) return 'A name is lower-case, such as station.example: no https://, port or path.';
  return null;
}

function portProblem(port: number, optional: boolean): string | null {
  if (optional && port === 0) return null;
  if (!Number.isInteger(port) || port < 1 || port > 65535) return optional ? 'A port is 1 to 65535, or 0 for off.' : 'A port is 1 to 65535.';
  return null;
}

/** One STUN server as both install scripts check it (their one pattern, and the port's range). */
const STUN_ENTRY = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*:[0-9]{1,5}$/;

/** A typed list of STUN servers read as weavesignal.sh and start.sh read `STUN_SERVERS`: split
 *  at commas and spaces, lower-cased, each `host:port` (a DNS name or an IPv4 address and a
 *  port from 1 to 65535, no `stun:` in front), repeats dropped, at most four. */
export function parseStunServers(text: string): { ok: true; servers: string[] } | { ok: false; message: string } {
  const servers: string[] = [];
  for (const raw of text.split(/[, ]+/).filter(Boolean)) {
    const entry = raw.toLowerCase();
    const port = Number(entry.slice(entry.lastIndexOf(':') + 1));
    if (!STUN_ENTRY.test(entry) || port < 1 || port > 65535) {
      return { ok: false, message: `“${raw}” is not host:port: a DNS name or an IPv4 address, a colon and a port, without stun: in front. For example stun.example.org:3478.` };
    }
    if (!servers.includes(entry)) servers.push(entry);
  }
  if (servers.length > MAX_STUN_SERVERS) return { ok: false, message: 'At most four STUN servers. Leave some out.' };
  return { ok: true, servers };
}

/** The STUN servers the answers name, in order; none unless one is chosen. */
export function stunServersOf(a: Pick<Answers, 'stun' | 'stunOther'>): string[] {
  if (a.stun === 'other') {
    const parsed = parseStunServers(a.stunOther);
    return parsed.ok ? parsed.servers : [];
  }
  return [...(STUN_CHOICES.find((c) => c.key === a.stun)?.servers ?? [])];
}

function stunProblem(a: Answers): Problem | null {
  if (!STUN_CHOICES.some((c) => c.key === a.stun)) return { field: 'stun', message: 'Choose a STUN server from the list, or None.' };
  if (a.stun !== 'other') return null;
  const parsed = parseStunServers(a.stunOther);
  if (!parsed.ok) return { field: 'stunOther', message: parsed.message };
  if (!parsed.servers.length) return { field: 'stunOther', message: 'Name the server as host:port, such as stun.example.org:3478, or choose None.' };
  return null;
}

/** The consent words' problem for the route's settings file, or `null`. Empty is the shipped
 *  words. The Compose bundle and the Quadlets share weavesignal.sh, which refuses a backslash,
 *  a dollar sign and a per cent sign (a Quadlets unit would read them as its own); the Cloudron
 *  package's file is shell, where each is escaped instead. Every route keeps the words on one line. */
export function consentProblem(text: string, route: Route): string | null {
  const t = text.trim();
  if (!t) return null;
  if (CONTROL.test(t)) return 'Keep the consent words on one line: no line break or tab.';
  if (route !== 'cloudron' && /[\\$%]/.test(t)) {
    return 'On this install route the consent words cannot hold a backslash, a dollar sign or a per cent sign. Leave them out: write “per cent”, or the currency’s name.';
  }
  return null;
}

/** Every `{word}` in the consent words that the station does not fill in (the control plane's
 *  `unknown_placeholders`): a caller would read it as written. */
export function unknownFillIns(text: string): string[] {
  const known = CONSENT_FILL_INS.map((f) => f.token);
  const found: string[] = [];
  for (const m of text.matchAll(/\{[a-z_]+\}/g)) if (!known.includes(m[0]) && !found.includes(m[0])) found.push(m[0]);
  return found;
}

/** The consent words as weavesignal.sh keeps them: single quotes, each apostrophe as `\'`. */
const composeConsent = (text: string) => `'${text.trim().replaceAll("'", "\\'")}'`;
/** The consent words for a file bash sources: double quotes, with the four characters the shell
 *  acts on inside them (backslash, double quote, dollar sign, backtick) escaped. */
const shellConsent = (text: string) => `"${text.trim().replace(/[\\"$`]/g, '\\$&')}"`;

const isFileRoute = (a: Answers) => a.route !== 'cloudron';
const goesPublic = (a: Answers) => a.route === 'cloudron' || a.isPublic;

/** Every problem that would make the output wrong. With any, [`generate`] gives no output. */
export function problems(a: Answers): Problem[] {
  const out: Problem[] = [];
  const add = (field: Field, message: string | null) => {
    if (message) out.push({ field, message });
  };
  add('name', nameProblem(a.name));
  add('contact', contactProblem(a.contact));
  if (a.route === 'cloudron') {
    add('studioDomain', domainProblem(a.studioDomain, 'the studio its name'));
    add('publicDomain', domainProblem(a.publicDomain, 'the public pages their name'));
    add('listenDomain', domainProblem(a.listenDomain, 'the listener stream its name'));
    add('roomDomain', domainProblem(a.roomDomain, 'the live room its name'));
    const names = [a.studioDomain, a.publicDomain, a.listenDomain, a.roomDomain].map((d) => d.trim());
    const fields: Field[] = ['studioDomain', 'publicDomain', 'listenDomain', 'roomDomain'];
    names.forEach((n, i) => {
      if (n && names.indexOf(n) < i) add(fields[i], 'Each of the four needs a name of its own.');
    });
    if (a.mail !== 'cloudron' && a.mail !== 'outbox' && a.mail !== 'off') add('mail', 'Choose how sign-in codes are sent.');
  } else {
    if (a.isPublic) {
      add('domain', domainProblem(a.domain, 'the station its name'));
      if (a.acmeEmail.trim() && !EMAIL.test(a.acmeEmail.trim())) add('acmeEmail', 'That is not an email address.');
    }
    if (a.mail === 'cloudron') add('mail', "Cloudron's mail is for the Cloudron route.");
    if (!GPU_CHOICES.some((c) => c.key === a.gpu)) add('gpu', 'Choose how the desk encodes video: look for a card, an NVIDIA card, or none.');
    if (a.mail !== 'off') {
      const from = a.mailFrom.trim();
      if (!from) add('mailFrom', 'Give the address sign-in codes come from.');
      else if (!EMAIL.test(from) || FILE_UNSAFE.test(from)) add('mailFrom', 'That is not an email address a settings file can hold.');
    }
  }
  if (a.nodeIp.trim() && !IPV4.test(a.nodeIp.trim())) add('nodeIp', "Give the server's public IPv4 address, such as 203.0.113.7, or leave it empty.");
  if (!PRESETS.some((p) => p.key === a.jurisdiction)) add('jurisdiction', 'Choose a jurisdiction from the list.');
  if (goesPublic(a)) {
    add('mediaPort', portProblem(a.mediaPort, false));
    add('turnTlsPort', portProblem(a.turnTlsPort, false));
    add('turnUdpPort', portProblem(a.turnUdpPort, true));
    add('iceTcpPort', portProblem(a.iceTcpPort, true));
    add('sourceIcecastPort', portProblem(a.sourceIcecastPort, true));
    add('sourceSrtPort', portProblem(a.sourceSrtPort, true));
    if (a.route === 'cloudron') {
      // Cloudron's own TURN holds these (ADR-0106; the package's manifest says the same).
      if (a.turnTlsPort === 5349) add('turnTlsPort', "Cloudron's own TURN holds 5349. Use 15349 or another free port.");
      if (a.turnUdpPort === 3478) add('turnUdpPort', "Cloudron's own TURN holds 3478. Use 3479 or another free port.");
      if (a.mediaPort >= 50000 && a.mediaPort <= 51000) add('mediaPort', "50000 to 51000 is Cloudron's own TURN relay range. Choose a port outside it.");
    } else if (a.turnTlsPort === 443) {
      add('turnTlsPort', 'The proxy holds 443. TURN over TLS needs a port of its own.');
    }
  }
  if (a.accent.trim() && !normaliseHex(a.accent)) add('accent', 'A colour is six hex digits, such as #b4532a.');
  // A station for this machine only names no STUN server: nobody outside reaches it.
  const stun = goesPublic(a) ? stunProblem(a) : null;
  if (stun) out.push(stun);
  add('consentText', consentProblem(a.consentText, a.route));
  return out;
}

export interface DnsRecord {
  name: string;
  type: 'A' | 'AAAA';
  /** What the record points at, in words: the wizard never asks for the address. */
  value: string;
  /**
   * `keep`: create it and leave it. `install-only`: Cloudron needs it to install; delete it once
   * the app runs. `never`: this name must have no such record.
   */
  when: 'keep' | 'install-only' | 'never';
  why: string;
}

export interface PortToOpen {
  port: number;
  proto: 'TCP' | 'UDP';
  what: string;
}

export interface Step {
  title: string;
  /** A sentence or two; plain text. */
  text?: string;
  /** Shell, to be shown in a copyable block. */
  command?: string;
}

export interface OutFile {
  /** The name to save it under. */
  name: string;
  /** Where it goes. */
  path: string;
  text: string;
}

export interface Output {
  file: OutFile;
  /** Cloudron: what to type into the install dialog, row by row. */
  dialog: { label: string; value: string }[];
  dns: DnsRecord[];
  ports: PortToOpen[];
  steps: Step[];
  /** What the station's setup page takes. */
  answers: OutFile;
  /** Where the setup page will be. */
  setupUrl: string;
  /** Things to know that are neither a record nor a command. */
  notes: string[];
}

export const REPOSITORY = 'https://forgejo.weavesignal.top/WeaveSignal/weavesignal.git';
export const CLOUDRON_CATALOGUE = 'https://forgejo.weavesignal.top/WeaveSignal/weavesignal/raw/branch/main/deploy/cloudron/CloudronVersions.json';
const IPV4_WORDS = "the server's public IPv4 address";
const IPV6_WORDS = "the server's IPv6 address";
const TURN_WHY =
  "This is the name guests' browsers reach TURN at. LiveKit's TURN refuses a guest who reaches it over IPv6, so the name must resolve to IPv4 only.";

const bool = (b: boolean) => (b ? 'true' : 'false');

/** The keys the Compose route's file may hold from this wizard (the test checks them against weavesignal.sh). */
function composeEnv(a: Answers): string {
  const lines: string[] = [
    '# WeaveSignal settings, made by the install wizard (https://weavesignal.top/docs/install-wizard/).',
    '# weavesignal.sh adds the secrets and the ports to this file and keeps every line here.',
    `STATION_NAME=${a.name.trim()}`,
    '',
    '# How a caller asks for their call to be taken down; call-in stays closed while it is empty.',
    `CONSENT_CONTACT=${a.contact.trim()}`,
    ...(a.consentText.trim()
      ? [
          '# The consent words a caller agrees to, in place of the shipped two sentences. In single',
          "# quotes, each apostrophe written \\'; {station}, {kept}, {contact} and {presenter} are filled in.",
          `CONSENT_TEXT=${composeConsent(a.consentText)}`,
        ]
      : []),
    '',
    '# Sign-in codes by email: off (staff use passwords, the chat is read-only) or outbox (codes',
    '# are written as files, for a test).',
    `MAIL_MODE=${a.mail}`,
  ];
  if (a.mail !== 'off') lines.push(`MAIL_FROM=${a.mailFrom.trim()}`);
  lines.push(
    '',
    '# Whether search engines and radio directories may list the station.',
    `SEARCH_LISTING=${bool(a.searchListing)}`,
    `DIRECTORY_LISTING=${bool(a.directoryListing)}`,
    '',
    "# The desk's video path: cameras, the vision mixer, a picture in the player.",
    `VIDEO=${bool(a.video)}`,
  );
  if (a.video && a.gpu !== 'auto') {
    lines.push(
      "# A GPU for the desk's video: nvidia hands the card to the desk by its CDI name (NVENC), none",
      '# keeps it on the CPU (SVT-AV1). Without this line weavesignal.sh looks for a card itself.',
      `GPU=${a.gpu}`,
    );
  }
  if (a.isPublic) {
    lines.push('', '# The public route (weavesignal.sh --public adds the name itself).');
    if (a.acmeEmail.trim()) lines.push(`ACME_EMAIL=${a.acmeEmail.trim()}`);
    if (a.nodeIp.trim()) lines.push(`NODE_IP=${a.nodeIp.trim()}`);
    const d = defaults(a.route);
    if (a.mediaPort !== d.mediaPort) lines.push(`LIVEKIT_MEDIA_PORT=${a.mediaPort}`);
    if (a.turnTlsPort !== d.turnTlsPort) lines.push(`TURN_TLS_PORT=${a.turnTlsPort}`);
    if (a.turnUdpPort !== 0) lines.push(`TURN_UDP_PORT=${a.turnUdpPort}`);
    if (a.iceTcpPort !== 0) lines.push(`LIVEKIT_TCP_PORT=${a.iceTcpPort}`);
    if (a.sourceIcecastPort !== 0) lines.push(`SOURCE_ICECAST_PORT=${a.sourceIcecastPort}`);
    if (a.sourceSrtPort !== 0) lines.push(`SOURCE_SRT_PORT=${a.sourceSrtPort}`);
    const stun = stunServersOf(a);
    if (stun.length) {
      lines.push('', '# STUN servers the station may ask (LiveKit, the desk and the join pages); without this', '# line it contacts none.', `STUN_SERVERS=${stun.join(',')}`);
    }
  }
  return `${lines.join('\n')}\n`;
}

/** The Cloudron package's settings file, whole: start.sh's first file with the answers in it. */
function cloudronEnv(a: Answers): string {
  const q = (v: string) => `"${v}"`;
  const lines = [
    '# WeaveSignal settings, made by the install wizard (https://weavesignal.top/docs/install-wizard/).',
    "# Restart the app after editing. The domains and the public ports are chosen in Cloudron's",
    '# dashboard, not here; the secrets are in secrets.env. A later package adds new settings below.',
    `STATION_NAME=${q(a.name.trim())}`,
    'ROOM_NAME=studio',
    '# Listener stream access: "off" leaves the stream open but unlisted; "on" asks for the user',
    '# `listener` and the password in secrets.env (the player page cannot play it then).',
    `LISTENERS_AUTH=${a.streamPassword ? 'on' : 'off'}`,
    'LISTENERS_MOUNT=programme.opus',
    "# LiveKit's public IPv4 address for ICE candidates. Empty: the A record of the room's domain, read",
    "# at every start; no STUN server is asked. Set it where that is not the address guests reach.",
    `NODE_IP=${a.nodeIp.trim()}`,
    '# Internal ports, on loopback inside the container. Change only on a clash.',
    'CONTROL_PORT=7310',
    'LISTENERS_PORT=7312',
    'SRT_INGEST_PORT=7313',
    'LIVEKIT_HTTP_PORT=7880',
    'LOG_LEVEL=info',
    '',
    '# How a caller asks for their call to be taken down; call-in stays closed while it is empty.',
    `CONSENT_CONTACT=${q(a.contact.trim())}`,
    ...(a.consentText.trim()
      ? [
          '# The consent words a caller agrees to, in place of the shipped two sentences. In double',
          '# quotes, with a backslash before each " $ ` and backslash; {station}, {kept}, {contact} and',
          '# {presenter} are filled in.',
          `CONSENT_TEXT=${shellConsent(a.consentText)}`,
        ]
      : []),
    ...(stunServersOf(a).length
      ? [
          '# STUN servers the station may ask (LiveKit, the desk and the join pages); without this',
          '# line it contacts none.',
          `STUN_SERVERS=${q(stunServersOf(a).join(','))}`,
        ]
      : []),
    "# Sign-in codes: cloudron (Cloudron's mail), outbox (files in /app/data/chat-outbox), or off.",
    `MAIL_MODE=${a.mail}`,
    '# Whether search engines and radio directories may list the station.',
    `SEARCH_LISTING=${bool(a.searchListing)}`,
    `DIRECTORY_LISTING=${bool(a.directoryListing)}`,
    "# The desk's video path: cameras, the vision mixer, a picture in the player.",
    `VIDEO=${bool(a.video)}`,
  ];
  return `${lines.join('\n')}\n`;
}

function answersFile(a: Answers): OutFile {
  const body = {
    weavesignal_setup: 1,
    name: a.name.trim(),
    contact: a.contact.trim(),
    jurisdiction: a.jurisdiction,
    accent: normaliseHex(a.accent) ?? '',
  };
  return {
    name: 'weavesignal-setup.json',
    path: "your own computer; the station's setup page opens it",
    text: `${JSON.stringify(body, null, 2)}\n`,
  };
}

/** What an answers file holds, checked: the setup page reads a file a person hands it. */
export interface SetupAnswers {
  name: string;
  contact: string;
  jurisdiction: string;
  accent: string;
}

/** Read an answers file. Throws a sentence for the person when it is not one. */
export function readAnswersFile(text: string): SetupAnswers {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    throw new Error('That file is not an answers file from the install wizard.');
  }
  const o = v as Record<string, unknown>;
  if (typeof v !== 'object' || v === null || o.weavesignal_setup !== 1) {
    throw new Error('That file is not an answers file from the install wizard.');
  }
  const text_ = (k: string, max: number) => {
    const s = typeof o[k] === 'string' ? (o[k] as string) : '';
    return [...s].slice(0, max).join('').replace(/[\u0000-\u001f\u007f]/g, '');
  };
  const jurisdiction = text_('jurisdiction', 40);
  return {
    name: text_('name', NAME_MAX),
    contact: text_('contact', CONTACT_MAX),
    jurisdiction: PRESETS.some((p) => p.key === jurisdiction) ? jurisdiction : '',
    accent: normaliseHex(text_('accent', 7)) ?? '',
  };
}

function dnsFor(a: Answers): DnsRecord[] {
  if (a.route === 'cloudron') {
    const names: [string, string][] = [
      [a.studioDomain.trim(), 'The studio (the app’s own domain), and TURN.'],
      [a.publicDomain.trim(), 'The player, call-in and the chat.'],
      [a.listenDomain.trim(), 'The listener stream.'],
      [a.roomDomain.trim(), 'The live room’s signalling.'],
    ];
    const out: DnsRecord[] = names.map(([name, why]) => ({ name, type: 'A', value: IPV4_WORDS, when: 'keep', why }));
    if (a.ipv6) {
      names.forEach(([name], i) => {
        out.push(
          i === 0
            ? { name, type: 'AAAA', value: IPV6_WORDS, when: 'install-only', why: `Cloudron waits for it at install. Delete it once the app runs. ${TURN_WHY}` }
            : { name, type: 'AAAA', value: IPV6_WORDS, when: 'keep', why: 'Cloudron waits for it at install; it can stay.' },
        );
      });
    } else {
      out.push({ name: names[0][0], type: 'AAAA', value: 'nothing', when: 'never', why: TURN_WHY });
    }
    return out;
  }
  if (!a.isPublic) return [];
  const name = a.domain.trim();
  return [
    { name, type: 'A', value: IPV4_WORDS, when: 'keep', why: 'One name serves everything: the player, the studio, the stream, the room and TURN.' },
    { name, type: 'AAAA', value: 'nothing', when: 'never', why: TURN_WHY },
  ];
}

function portsFor(a: Answers): PortToOpen[] {
  if (!goesPublic(a)) return [];
  const out: PortToOpen[] = [];
  if (isFileRoute(a)) {
    out.push(
      { port: 80, proto: 'TCP', what: 'the proxy: the certificate check, and the redirect to HTTPS' },
      { port: 443, proto: 'TCP', what: 'the proxy: the station' },
    );
  }
  out.push(
    { port: a.mediaPort, proto: 'UDP', what: "the live room's media" },
    { port: a.turnTlsPort, proto: 'TCP', what: 'TURN over TLS, for guests whose networks block UDP' },
  );
  if (a.turnUdpPort) out.push({ port: a.turnUdpPort, proto: 'UDP', what: 'TURN over UDP' });
  if (a.iceTcpPort) out.push({ port: a.iceTcpPort, proto: 'TCP', what: "the live room's media over TCP" });
  if (a.sourceIcecastPort) out.push({ port: a.sourceIcecastPort, proto: 'TCP', what: "hosts' own studios over Icecast" });
  if (a.sourceSrtPort) out.push({ port: a.sourceSrtPort, proto: 'UDP', what: "hosts' own studios over SRT" });
  return out;
}

const SETUP_STEP_TEXT =
  'Sign in with the station key, then choose Open an answers file and pick weavesignal-setup.json. The page fills in the jurisdiction and the colour, and takes the banner and the icon.';

function composeSteps(a: Answers, setupUrl: string): Step[] {
  const pub = a.isPublic ? ` --public ${a.domain.trim()}` : '';
  const steps: Step[] = [];
  if (a.isPublic) {
    steps.push({ title: 'Create the DNS record', text: 'Create the A record in the list above and wait until the name answers with the server’s address. Check that it has no AAAA record.', command: `dig +short A ${a.domain.trim()}\ndig +short AAAA ${a.domain.trim()}   # must print nothing` });
    steps.push({ title: 'Open the ports', text: 'Open the ports in the list above in the server’s firewall and in any cloud firewall in front of it.' });
  }
  steps.push({ title: 'Get WeaveSignal', text: 'On the server, as the user who will run the station. The folder stays where you put it.', command: `git clone ${REPOSITORY}\ncd weavesignal` });
  if (a.route === 'compose') {
    steps.push({
      title: 'Save the settings file',
      text: 'Save the file above as deploy/compose/.env, readable by you only.',
      command: 'nano deploy/compose/.env        # paste the file, save\nchmod 600 deploy/compose/.env',
    });
    steps.push({
      title: 'Install and start',
      text: `The script makes the station’s secrets, adds them to the file and starts the station${a.isPublic ? ', with a certificate for the name' : ''}. It ends by saying where to sign in.`,
      command: `deploy/compose/weavesignal.sh${pub}`,
    });
  } else {
    steps.push({
      title: 'Save the settings file',
      text: 'Save the file above as ~/.config/weavesignal/station.env, readable by you only.',
      command: 'mkdir -p ~/.config/weavesignal\nnano ~/.config/weavesignal/station.env        # paste the file, save\nchmod 600 ~/.config/weavesignal/station.env',
    });
    steps.push({
      title: 'Make the secrets and choose the ports',
      text: 'The script completes the file without starting anything.',
      command: `deploy/compose/weavesignal.sh --no-start --env-file ~/.config/weavesignal/station.env${pub}`,
    });
    steps.push({
      title: 'Generate the units and start',
      command:
        'deploy/quadlets/generate.sh --env-file ~/.config/weavesignal/station.env \\\n  --out ~/.config/weavesignal/quadlets --create-secrets\nmkdir -p ~/.config/containers/systemd ~/.config/systemd/user\ncp ~/.config/weavesignal/quadlets/containers/systemd/* ~/.config/containers/systemd/\ncp ~/.config/weavesignal/quadlets/systemd/* ~/.config/systemd/user/\nsystemctl --user daemon-reload\nsystemctl --user enable --now weavesignal.target\nloginctl enable-linger',
    });
  }
  const envPath = a.route === 'compose' ? 'deploy/compose/.env' : '~/.config/weavesignal/station.env';
  steps.push({
    title: 'Read the station key',
    text: 'The script wrote it into the settings file. It is the station’s own credential: keep it private.',
    command: `grep '^CONTROL_ADMIN_TOKEN=' ${envPath}`,
  });
  steps.push({ title: 'Finish on the station’s setup page', text: `Open ${setupUrl}. ${SETUP_STEP_TEXT}` });
  return steps;
}

function cloudronSteps(a: Answers, setupUrl: string): Step[] {
  const studio = a.studioDomain.trim();
  const steps: Step[] = [];
  if (a.ipv6) {
    steps.push({
      title: 'Set the domain to manual DNS',
      text: 'In Cloudron’s dashboard, under Domains, set the domain’s DNS provider to Manual. With a provider set, Cloudron writes the studio name’s AAAA record back by itself.',
    });
    steps.push({
      title: 'Create the DNS records',
      text: 'Create every record in the list above, the AAAA records included: Cloudron waits at install until each name has both.',
    });
  } else {
    steps.push({
      title: 'Create the DNS records',
      text: 'Create the A records in the list above. A Cloudron that manages the domain’s DNS creates them itself at install.',
    });
  }
  steps.push({ title: 'Open the ports', text: 'Open the ports in the list above in the server’s firewall and in any cloud firewall in front of it.' });
  const secondary = `LISTENERS_DOMAIN=${a.listenDomain.trim()},ROOM_DOMAIN=${a.roomDomain.trim()},PUBLIC_DOMAIN=${a.publicDomain.trim()}`;
  const portBindings = [`LIVEKIT_MEDIA_PORT=${a.mediaPort}`, `TURN_TLS_PORT=${a.turnTlsPort}`];
  if (a.turnUdpPort) portBindings.push(`TURN_UDP_PORT=${a.turnUdpPort}`);
  if (a.iceTcpPort) portBindings.push(`LIVEKIT_TCP_PORT=${a.iceTcpPort}`);
  if (a.sourceIcecastPort) portBindings.push(`SOURCE_ICECAST_PORT=${a.sourceIcecastPort}`);
  if (a.sourceSrtPort) portBindings.push(`SOURCE_SRT_PORT=${a.sourceSrtPort}`);
  steps.push({
    title: 'Install the app',
    text: 'In Cloudron’s dashboard: App Store, Community apps, paste the catalogue address, Install, and fill the dialog in from the table above. Or, with the Cloudron command-line tool logged in:',
    command: `cloudron install --versions-url ${CLOUDRON_CATALOGUE} \\\n  --location ${studio} \\\n  --secondary-domains ${secondary} \\\n  --port-bindings ${portBindings.join(',')}`,
  });
  if (a.ipv6) {
    steps.push({
      title: 'Delete the studio name’s AAAA record',
      text: `Once the app is running, delete the AAAA record of ${studio} and keep the others. Do it again whenever Cloudron checks the names: after a change of domains, or a restore.`,
      command: `dig +short AAAA ${studio}   # must print nothing`,
    });
  }
  steps.push({
    title: 'Put the settings file in place',
    text: 'In the app’s File Manager, open /app/data/weavesignal.env, replace what is there with the file above, save, and restart the app. Or from the command line:',
    command: `cloudron push --app ${studio} weavesignal.env /app/data/weavesignal.env\ncloudron restart --app ${studio}`,
  });
  steps.push({
    title: 'Read the station key',
    text: 'The app made it at its first start, in /app/data/secrets.env (the File Manager shows it too). It is the station’s own credential: keep it private.',
    command: `cloudron exec --app ${studio} -- grep '^CONTROL_ADMIN_TOKEN=' /app/data/secrets.env`,
  });
  steps.push({ title: 'Finish on the station’s setup page', text: `Open ${setupUrl}. ${SETUP_STEP_TEXT}` });
  return steps;
}

/** The output for a set of answers, or the problems that stop it. */
export function generate(a: Answers): { ok: true; output: Output } | { ok: false; problems: Problem[] } {
  const found = problems(a);
  if (found.length) return { ok: false, problems: found };
  const notes: string[] = [];
  let file: OutFile;
  let setupUrl: string;
  let steps: Step[];
  let dialog: Output['dialog'] = [];
  if (a.route === 'cloudron') {
    file = { name: 'weavesignal.env', path: '/app/data/weavesignal.env, in the app’s File Manager', text: cloudronEnv(a) };
    setupUrl = `https://${a.studioDomain.trim()}/setup/`;
    steps = cloudronSteps(a, setupUrl);
    dialog = [
      { label: 'Location (the app’s own domain)', value: a.studioDomain.trim() },
      { label: 'Listener stream', value: a.listenDomain.trim() },
      { label: 'Live room signalling', value: a.roomDomain.trim() },
      { label: 'Public pages', value: a.publicDomain.trim() },
      { label: 'Live room media (UDP)', value: String(a.mediaPort) },
      { label: 'TURN over TLS', value: String(a.turnTlsPort) },
      { label: 'TURN over UDP', value: a.turnUdpPort ? String(a.turnUdpPort) : 'off' },
      { label: 'Live room media over TCP', value: a.iceTcpPort ? String(a.iceTcpPort) : 'off' },
      { label: "Hosts' own studios: Icecast", value: a.sourceIcecastPort ? String(a.sourceIcecastPort) : 'off' },
      { label: "Hosts' own studios: SRT", value: a.sourceSrtPort ? String(a.sourceSrtPort) : 'off' },
    ];
    if (a.streamPassword) {
      notes.push('With the stream’s password on, the player page stays silent: a browser does not ask for a password for audio from another name. Players and apps given the address, the user listener and LISTENERS_PASSWORD from secrets.env can play it.');
    }
    if (a.ipv6) notes.push('Without its AAAA record the studio is reached over IPv4 only. The player, the stream and the room keep IPv6.');
    if (a.video) notes.push('Video needs the app’s memory limit at 2 GiB (the package’s default from 1.0.1) and a server that is not busy.');
  } else {
    const where = a.route === 'compose' ? 'deploy/compose/.env' : '~/.config/weavesignal/station.env';
    file = { name: a.route === 'compose' ? '.env' : 'station.env', path: `${where}, on the server`, text: composeEnv(a) };
    setupUrl = a.isPublic ? `https://${a.domain.trim()}/setup/` : 'http://127.0.0.1:7310/setup/';
    steps = composeSteps(a, setupUrl);
    if (!a.isPublic) notes.push('Every address is on this machine only: nobody else can listen or call. Run the script again with --public and a name when the station should go out.');
    if (a.ipv6 && a.isPublic) notes.push('The station is reached over IPv4 only, because its one name is also the TURN name.');
    if (a.mail === 'outbox') notes.push('Outbox writes each sign-in code to a file instead of sending it: for a test, not for listeners.');
    if (a.video && a.gpu === 'nvidia') {
      notes.push('The card reaches the desk through its CDI spec: install the NVIDIA Container Toolkit and run sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml before the script, which stops without it.');
    }
  }
  if (!a.contact.trim()) notes.push('Call-in stays closed until the station has a takedown contact. The setup page can add it later.');
  const stun = goesPublic(a) ? stunServersOf(a) : [];
  if (stun.length) {
    const one = stun.length === 1;
    notes.push(
      `The station, and each guest’s and caller’s browser, will ask ${stun.join(', ')} for its own public address. Whoever runs ${one ? 'that server' : 'each server'} sees those addresses and when they ask, and nothing else. ${one ? 'It is' : 'They are'} not ours, and nobody has promised ${one ? 'it' : 'them'} to your station.`,
    );
  }
  if (a.consentText.trim()) {
    const unknown = unknownFillIns(a.consentText);
    if (unknown.length) notes.push(`Your consent words hold ${unknown.join(' and ')}, which the station does not fill in: callers would read ${unknown.length === 1 ? 'it' : 'them'} as written. The fill-ins are {station}, {kept}, {contact} and {presenter}.`);
    if (!a.consentText.includes('{contact}')) notes.push('Your consent words have no {contact}, so callers are not told how to ask for their call to be taken down.');
    notes.push('The station shows your consent words in place of the shipped ones. Change them later and every caller is asked to agree again.');
  }
  if (a.jurisdiction === 'none') notes.push('No retention keeps nothing after a show. A licensed broadcaster must choose its jurisdiction; the setup page explains each one.');
  return {
    ok: true,
    output: { file, dialog, dns: dnsFor(a), ports: portsFor(a), steps, answers: answersFile(a), setupUrl, notes },
  };
}

/** The keys a generated settings file sets, in order. */
export function keysOf(text: string): string[] {
  return text
    .split('\n')
    .map((l) => /^([A-Z_][A-Z0-9_]*)=/.exec(l)?.[1])
    .filter((k): k is string => !!k);
}
