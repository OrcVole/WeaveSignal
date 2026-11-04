// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The update screen against a fake Rust side (src-tauri/src/update.rs): what a person can ask
// for and when the desk's state stops them. Tauri's core is replaced by a fake that answers as
// the four update commands do.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import type { Source, View } from './lib/api.ts';
import type { UpdateCheck } from './lib/update.ts';

const calls: string[] = [];
let answer: (cmd: string) => unknown = () => null;
let channels: FakeChannel[] = [];

class FakeChannel {
  onmessage: (m: unknown) => void = () => {};
  constructor() {
    channels.push(this);
  }
}

vi.mock('@tauri-apps/api/core', () => ({
  Channel: FakeChannel,
  invoke: async (cmd: string) => {
    calls.push(cmd);
    return answer(cmd);
  },
}));

const OFFER: UpdateCheck = {
  enabled: true,
  current: '0.1.0',
  available: { version: '0.2.0', notes: 'The update screen.', date: 1_793_620_000, skipped: false, downloaded: false },
  previous: null,
};

function open(on: boolean): View {
  const s = {
    identity: 'moira', name: 'Moira Fraser', role: 'presenter', on_air: on, mute: false, cue: false, talk: false,
    talk_to: null, present: true, away_ms: null, linked: true, lufs: null, lufs_peak: null, slot: 1, caller: null,
  } as Source;
  return { sources: [s], desk: { reachable: true, error: null, playout: { connected: true, for_ms: null } } } as View;
}

beforeEach(() => {
  calls.length = 0;
  channels = [];
  vi.stubGlobal('location', { protocol: 'tauri:', hostname: 'localhost', search: '' });
  vi.resetModules();
});
afterEach(() => vi.unstubAllGlobals());

async function load() {
  const u = await import('./lib/update.svelte.ts');
  const { studio } = await import('./lib/studio.svelte.ts');
  return { ...u, studio };
}

describe('checking', () => {
  it('keeps what the feed offers and points the frame at it', async () => {
    const { checkNow, updates, updateNag } = await load();
    answer = () => structuredClone(OFFER);
    await checkNow();
    expect(calls).toEqual(['update_check']);
    expect(updates.check?.available?.version).toBe('0.2.0');
    expect(updateNag()).toBe('0.2.0');
    expect(updates.phase).toBe('idle');
  });

  it('does not nag about a version the person rolled back from', async () => {
    const { checkNow, updateNag } = await load();
    answer = () => ({ ...OFFER, available: { ...OFF_AVAILABLE(), skipped: true } });
    await checkNow();
    expect(updateNag()).toBeNull();
  });

  it('says a feed that cannot be reached, and stays usable', async () => {
    const { checkNow, updates } = await load();
    answer = () => {
      throw 'error sending request for url (https://updates.example.org/latest.json)';
    };
    await checkNow();
    expect(updates.error).toMatch(/^Could not check for updates: error sending request/);
    expect(updates.phase).toBe('idle');
  });

  it('does nothing in a browser build', async () => {
    vi.stubGlobal('location', { protocol: 'http:', hostname: '127.0.0.1', search: '' });
    vi.resetModules();
    const { checkNow, installUpdateCheck } = await load();
    await checkNow();
    installUpdateCheck(0)();
    expect(calls).toEqual([]);
  });
});

function OFF_AVAILABLE() {
  return structuredClone(OFFER).available!;
}

describe('installing', () => {
  it('downloads, then installs, when nothing is open', async () => {
    const { checkNow, installNow, updates } = await load();
    answer = (cmd) => (cmd === 'update_check' ? structuredClone(OFFER) : null);
    await checkNow();
    await installNow();
    expect(calls).toEqual(['update_check', 'update_download', 'update_install']);
    expect(updates.error).toBe('');
  });

  it('shows the download as it comes', async () => {
    const { checkNow, installNow, updates } = await load();
    let seen: unknown = null;
    answer = (cmd) => {
      if (cmd === 'update_check') return structuredClone(OFFER);
      if (cmd === 'update_download') {
        channels.at(-1)!.onmessage({ done: 40, total: 80 });
        seen = { ...updates.progress };
      }
      return null;
    };
    await checkNow();
    await installNow();
    expect(seen).toEqual({ done: 40, total: 80 });
    expect(updates.progress).toBeNull();
  });

  it('asks for nothing while a strip is open', async () => {
    const { checkNow, installNow, updates, studio } = await load();
    answer = () => structuredClone(OFFER);
    await checkNow();
    studio.view = open(true);
    await installNow();
    expect(calls).toEqual(['update_check']);
    expect(updates.refused).toMatch(/on air: 1 strip is open/);
  });

  it('asks for nothing while a talk key is held', async () => {
    const { checkNow, installNow, updates, studio } = await load();
    answer = () => structuredClone(OFFER);
    await checkNow();
    studio.view = open(false);
    studio.talking = ['moira'];
    await installNow();
    expect(calls).toEqual(['update_check']);
    expect(updates.refused).toMatch(/talk key is held/);
    studio.talking = [];
  });

  it('does not install a download when a strip opened while it came down, and installs later without downloading again', async () => {
    const { checkNow, installNow, updates, studio } = await load();
    studio.view = open(false);
    answer = (cmd) => {
      if (cmd === 'update_check') return structuredClone(OFFER);
      if (cmd === 'update_download') studio.view = open(true); // the show starts mid-download
      return null;
    };
    await checkNow();
    await installNow();
    expect(calls).toEqual(['update_check', 'update_download']);
    expect(updates.check?.available?.downloaded).toBe(true);
    expect(updates.refused).toMatch(/on air/);
    expect(updates.phase).toBe('idle');

    studio.view = open(false);
    await installNow();
    expect(calls).toEqual(['update_check', 'update_download', 'update_install']);
    expect(updates.refused).toBe('');
  });

  it('reports a failed install and lets the person try again', async () => {
    const { checkNow, installNow, updates } = await load();
    answer = (cmd) => {
      if (cmd === 'update_check') return structuredClone(OFFER);
      if (cmd === 'update_install') throw 'signature verification failed';
      return null;
    };
    await checkNow();
    await installNow();
    expect(updates.error).toBe('The update was not installed: signature verification failed');
    expect(updates.phase).toBe('idle');
  });
});

describe('rolling back', () => {
  const KEPT: UpdateCheck = { ...OFFER, available: null, previous: '0.1.0' };

  it('goes back when nothing is open', async () => {
    const { checkNow, rollbackNow } = await load();
    answer = () => structuredClone(KEPT);
    await checkNow();
    await rollbackNow();
    expect(calls).toEqual(['update_check', 'update_rollback']);
  });

  it('is refused while a strip is open', async () => {
    const { checkNow, rollbackNow, updates, studio } = await load();
    answer = () => structuredClone(KEPT);
    await checkNow();
    studio.view = open(true);
    await rollbackNow();
    expect(calls).toEqual(['update_check']);
    expect(updates.refused).not.toBe('');
  });

  it('has nothing to do when no earlier version is kept', async () => {
    const { checkNow, rollbackNow } = await load();
    answer = () => structuredClone(OFFER);
    await checkNow();
    await rollbackNow();
    expect(calls).toEqual(['update_check']);
  });
});

describe('the panel', () => {
  async function panel(check: UpdateCheck | null, view: View | null = null) {
    const { updates, studio } = await load();
    updates.check = check;
    studio.view = view;
    // The renderer and the component come from the same module graph, after the reset.
    const { render } = await import('svelte/server');
    const Updates = (await import('./settings/Updates.svelte')).default;
    return render(Updates).body;
  }

  it('is named, shows the notes as text, and offers the install', async () => {
    const html = await panel({ ...OFFER, available: { ...OFF_AVAILABLE(), notes: 'Fixes <b>the desk</b>.' } });
    expect(html).toContain('data-panel="settings.updates"');
    expect(PANELS['settings.updates']).toBeTruthy();
    expect(html).toContain('Version 0.2.0 is available.');
    expect(html).toContain('Fixes &lt;b>the desk&lt;/b>.');
    expect(html).toContain('Download and install');
    expect(html).not.toMatch(/data-testid="update-refusal"/);
  });

  it('says why the install waits, and disables it, while a strip is open', async () => {
    const html = await panel(OFFER, open(true));
    expect(html).toContain('data-testid="update-refusal"');
    expect(html).toContain('Updating waits until nothing is open.');
    expect(html).toMatch(/<button[^>]*disabled[^>]*>(<!--[^>]*-->)*Download and install/);
  });

  it('says a build without a key does not update', async () => {
    const html = await panel({ enabled: false, current: '0.1.0', available: null, previous: null });
    expect(html).toContain('does not update itself');
    expect(html).not.toContain('Download and install');
  });

  it('offers the rollback when an earlier version is kept', async () => {
    const html = await panel({ ...OFFER, available: null, previous: '0.1.0' });
    expect(html).toContain('You have the latest version.');
    expect(html).toContain('Roll back to 0.1.0');
  });

  it('is not drawn in a browser build', async () => {
    vi.stubGlobal('location', { protocol: 'http:', hostname: '127.0.0.1', search: '' });
    vi.resetModules();
    expect(await panel(OFFER)).not.toContain('settings.updates');
  });
});
