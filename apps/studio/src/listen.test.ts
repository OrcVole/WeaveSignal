// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Listening from the studio app (1.3.1; K1 to K3 and G11 to G13 of the first-sound run): the
// Desk's Listeners panel and Settings open the player; a window on the station key is told how to
// speak; the first account starts as Admin; a fixed address clears its own error.
import { render } from 'svelte/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DESK_B, PANELS, SETTINGS } from '../../../packages/ui/panels.ts';
import DeskB from './desk/DeskB.svelte';
import Settings from './settings/Settings.svelte';
import Staff from './settings/Staff.svelte';
import { account } from './lib/account.svelte.ts';
import { defaultRole } from './lib/account.ts';
import { NO_SCHEME, urlErrorAfter } from './lib/address.ts';
import { KEY_NOTE, keyNoteShown } from './lib/keyNote.ts';
import { PLAYER, firstMinute } from './lib/player.ts';
import { PLAYER_USED_KEY, markPlayerUsed, playerUsed } from './lib/playerUsed.svelte.ts';
import { sched } from './lib/schedule.svelte.ts';
import { studio } from './lib/studio.svelte.ts';
import { NO_SHOW } from './lib/steps.ts';
import type { DeskPerson, Me, Source, View } from './lib/api.ts';

const NOW = Date.UTC(2026, 10, 23, 9, 0, 0);

const eilidh: Source = {
  slot: 1, identity: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'presenter', on_air: true, mute: false, cue: false, talk: false, talk_to: null,
  present: true, away_ms: null, linked: true, lufs: -22.6, lufs_peak: -19.4, caller: null,
} satisfies DeskPerson & Pick<Source, 'slot' | 'caller'>;

function view(sources: Source[]): View {
  return {
    station: 'Kettle FM',
    now_ms: NOW,
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 14_000 } },
    seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
    sources,
    callers: [],
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null, held: 0, open_reports: 0 },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    step: { ...NO_SHOW, step: 'solo', may_change: true },
  };
}

const key: Me = { kind: 'station', role: 'admin' };
const morag: Me = { kind: 'staff', role: 'admin', account: { id: 'a1', username: 'morag', name: 'Morag Campbell', role: 'admin', disabled_ms: null, created_ms: 1 } };

beforeEach(() => {
  studio.tab = 'desk';
  studio.settings = false;
  studio.now = NOW;
  studio.skew = 0;
  account.me = null;
  sched.publicUrl = undefined;
});

describe('Open the player', () => {
  it('is in the Listeners panel with its sentence, the first thing a newcomer sees of listening', () => {
    sched.publicUrl = 'https://radio.example.org';
    studio.view = view([]);
    const { body } = render(DeskB);
    const at = body.indexOf('data-panel="desk.audience"');
    const panel = body.slice(at, body.indexOf('</section>', at));
    expect(panel).toContain(PLAYER.sound);
    expect(panel).toContain('Open the player');
    expect(panel).toContain('Copy link');
    expect(panel).not.toContain('aria-disabled="true"');
  });

  it('keeps its tooltip with the sentence, so the sentence can stand down after the first use', () => {
    sched.publicUrl = 'https://radio.example.org';
    studio.view = view([]);
    const { body } = render(DeskB);
    const at = body.indexOf('data-panel="desk.audience"');
    const panel = body.slice(at, body.indexOf('</section>', at));
    expect(panel).toContain('role="tooltip"');
    expect(panel).toContain(PLAYER.openTip);
  });

  it('waits, with its reason beside the buttons, until the station has said its address', () => {
    studio.view = view([]);
    const { body } = render(DeskB);
    expect(body).toContain('aria-disabled="true"');
    expect(body).toContain(PLAYER.asking);
  });

  it('is a panel in Settings for everyone, with the sentence and the address', () => {
    sched.publicUrl = 'https://radio.example.org';
    account.me = morag;
    const { body } = render(Settings, { props: { me: morag } });
    expect(body).toContain('data-panel="settings.player"');
    expect(body).toContain(PLAYER.sound);
    expect(body).toContain('Open the player');
  });
});

describe('the Listeners block gives its room back after the first use (design review F1)', () => {
  const kept = new Map<string, string>();
  beforeEach(() => {
    kept.clear();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => kept.get(k) ?? null,
      setItem: (k: string, v: string) => void kept.set(k, v),
    });
    sched.publicUrl = 'https://radio.example.org';
    studio.view = view([]);
  });
  afterEach(() => vi.unstubAllGlobals());

  const listenersPanel = (): string => {
    const { body } = render(DeskB);
    const at = body.indexOf('data-panel="desk.audience"');
    return body.slice(at, body.indexOf('</section>', at));
  };

  it('draws the sentence until this computer has opened or copied the player', () => {
    expect(playerUsed()).toBe(false);
    expect(listenersPanel()).toContain('data-testid="player-sound"');
  });

  it('then draws the two buttons alone, still named, with no sentence and no hairline above them', () => {
    markPlayerUsed();
    expect(kept.get(PLAYER_USED_KEY)).toBe('1');
    expect(playerUsed()).toBe(true);
    const panel = listenersPanel();
    expect(panel).not.toContain('data-testid="player-sound"');
    expect(panel).not.toContain(`<span class="small" data-testid`);
    expect(panel).toContain('Open the player');
    expect(panel).toContain('Copy link');
  });

  it('keeps Settings’ own sentence whatever has been used', () => {
    markPlayerUsed();
    account.me = morag;
    const { body } = render(Settings, { props: { me: morag } });
    expect(body).toContain(PLAYER.sound);
    expect(body).toContain('about twenty seconds behind the desk (the Desk’s clock shows the figure)');
    // A window's own address says so when it is copied (design review F7), and the live region stays in the page, empty.
    expect(PLAYER.copiedOwn).toBe('Copied. This address works on this computer or network only.');
    expect(body).toContain('class="said small');
  });
});

describe('the station key has no voice', () => {
  it('says how to speak, where the button would be, and goes to Staff', () => {
    account.me = key;
    studio.view = view([]);
    const { body } = render(DeskB);
    expect(body).toContain('data-testid="station-key-note"');
    expect(body).toContain(KEY_NOTE.text);
    expect(body).toContain('Open Staff');
    expect(body).not.toContain('sign out (your name');
    expect(body).not.toContain('join yourself with your own microphone link');
    // The first minute (G12): the carts and the player, in a line.
    expect(body).toContain('press Open the player in the Listeners panel, then Listen live on that page');
    expect(body).toContain(firstMinute('about 15 s behind (an estimate)'));
  });

  it('is a quiet seat of its own when someone is already on the stage', () => {
    account.me = key;
    studio.view = view([{ ...eilidh, role: 'guest' }]);
    const { body } = render(DeskB);
    expect(body).toContain('keynote');
    expect(body).toContain(KEY_NOTE.text);
    expect(body).not.toContain('then Listen live on that page');
  });

  it('is one quiet line in the Not on air head, with Open Staff, where that row is drawn (design review F2)', () => {
    account.me = key;
    const v = view([{ ...eilidh, role: 'guest' }]);
    studio.view = { ...v, step: { ...v.step, step: 'full' } };
    const { body } = render(DeskB);
    const from = body.indexOf('data-panel="desk.waiting"');
    expect(from).toBeGreaterThan(-1);
    const head = body.slice(from, body.indexOf('</div>', from));
    expect(head).toContain(KEY_NOTE.head);
    expect(head).toContain('data-panel="desk.station-key"');
    expect(head).toContain('Open Staff');
    // In place of the row's legend, and not also as a seat on the stage.
    expect(head).not.toContain('a source moves between the stage');
    expect(body.split('data-testid="station-key-note"').length - 1).toBe(1);
    expect(body).not.toContain('empty seat own keynote');
  });

  it('keeps its seat at Solo, which has no Not on air row', () => {
    account.me = key;
    studio.view = view([{ ...eilidh, role: 'guest' }]);
    const { body } = render(DeskB);
    expect(body).toContain('empty seat own keynote');
    expect(body).toContain(KEY_NOTE.text);
    expect(body).not.toContain('data-panel="desk.waiting"');
  });

  it('is not shown to a signed-in account, which has its own button, nor once a presenter is in the room', () => {
    account.me = morag;
    studio.view = view([]);
    expect(render(DeskB).body).not.toContain('station-key-note');
    account.me = key;
    studio.view = view([eilidh]);
    expect(render(DeskB).body).not.toContain('station-key-note');
  });

  it('only a window on the key, and only where the button would be', () => {
    expect(keyNoteShown(key, true)).toBe(true);
    expect(keyNoteShown(key, false)).toBe(false);
    expect(keyNoteShown(morag, true)).toBe(false);
    expect(keyNoteShown(null, true)).toBe(false);
  });
});

describe('the station key’s second step (design review F2)', () => {
  it('Staff says which button signs the key out, on a station-key window only', () => {
    const asKey = render(Staff, { props: { me: key, initial: [] } }).body;
    expect(asKey).toContain('data-testid="staff-key-steps"');
    expect(asKey).toContain('press <b>Station key</b>');
    const me: Me = { kind: 'staff', role: 'admin', account: { id: 'a-shona', username: 'shona', name: 'Shona Campbell', role: 'admin', disabled_ms: null, created_ms: 1 } as Me['account'] };
    expect(render(Staff, { props: { me, initial: [] } }).body).not.toContain('staff-key-steps');
  });
});

describe('small rough edges (G11)', () => {
  it('the first account starts as Admin, as the empty list says; everyone after as Producer', () => {
    expect(defaultRole(0)).toBe('admin');
    expect(defaultRole(1)).toBe('producer');
    expect(defaultRole(null)).toBe('producer'); // not read yet: never "no one yet"
    const { body } = render(Staff, { props: { me: key, initial: [] } });
    expect(body).toContain('Add the first admin');
  });

  it('a fixed address loses its error at once; a wrong one keeps its sentence', () => {
    expect(urlErrorAfter(NO_SCHEME, 'https://radio.example.org')).toBeUndefined();
    expect(urlErrorAfter(NO_SCHEME, 'radio.example.org')).toBe(NO_SCHEME);
    expect(urlErrorAfter(undefined, 'radio.example.org')).toBeUndefined();
  });
});

describe('the registry', () => {
  it('names the new panels', () => {
    for (const name of ['settings.player', 'desk.station-key']) expect(PANELS[name], name).toBeTruthy();
    // Neither is part of the lists whose every name is drawn at once: they come with a condition.
    expect(SETTINGS.map((p) => p.name)).not.toContain('settings.player');
    expect(DESK_B.map((p) => p.name)).not.toContain('desk.station-key');
  });
});
