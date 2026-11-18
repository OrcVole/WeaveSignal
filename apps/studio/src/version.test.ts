// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The version in the frame (the operator, 2026-11-17): the studio app's own, always; the
// station's beside it only when the two differ; both in full in the tooltip; and when the
// station's is read again. The frame is rendered on the server, as a browser build draws it
// (no Tauri), which is also the proof that it does not need one.
import { render } from 'svelte/server';
import { beforeEach, describe, expect, it } from 'vitest';
import { PANELS } from '../../../packages/ui/panels.ts';
import App from './App.svelte';
import { APP_VERSION } from './appVersion.ts';
import { studio } from './lib/studio.svelte.ts';
import { REREAD_MS, rereadNow, versionLabel } from './lib/version.ts';

/** The frame, and in it the version's own words and its tooltip. */
function frame(): { frame: string; label: string; tip: string } {
  const body = render(App).body;
  const frame = body.slice(body.indexOf('<header class="frame'), body.indexOf('</header>'));
  const label = frame.slice(frame.indexOf('<span class="ver'), frame.indexOf('<span id="ver-tip"'));
  const tip = frame.slice(frame.indexOf('<span id="ver-tip"'), frame.indexOf('<span class="account'));
  return { frame, label, tip };
}
const words = (html: string) => html.replace(/<!--.*?-->/g, '').replace(/<[^>]+>/g, '').replace(/&nbsp;|\u00a0/g, ' ').replace(/\s+/g, ' ').trim();

describe('the app’s own version', () => {
  it('is the one Tauri packages the app with, and reads as a version', () => {
    // Read from src-tauri/tauri.conf.json at build time: the number Settings › Updates shows.
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });
});

describe('what the frame prints', () => {
  it('prints the app’s version alone while the station’s is the same, or not known', () => {
    expect(versionLabel('1.1.0', null)).toEqual({ app: '1.1.0', station: null, stationShort: null, tip: ['This studio app is WeaveSignal 1.1.0.'] });
    expect(versionLabel('1.1.0', '1.1.0')).toEqual({ app: '1.1.0', station: null, stationShort: null, tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs the same version.'] });
    // Only the numbers count: a test build of the same release is not a difference, and the
    // tooltip still says exactly what the station runs.
    expect(versionLabel('1.1.0', '1.1.0-test.4')).toEqual({ app: '1.1.0', station: null, stationShort: null, tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs 1.1.0-test.4, the same release.'] });
  });

  it('prints the station’s beside it when the numbers differ, and says which way and what follows', () => {
    const older = versionLabel('1.1.0', '1.0.5');
    expect(older.station).toBe('1.0.5');
    expect(older.stationShort).toBe('1.0.5');
    expect(older.tip[1]).toBe('The station runs 1.0.5, older than this app: what it does not have yet is hidden or switched off here.');
    const newer = versionLabel('1.1.0', '1.2.0-test.1');
    expect(newer.station).toBe('1.2.0-test.1');
    // Where the frame has little room it prints the numbers alone.
    expect(newer.stationShort).toBe('1.2.0');
    expect(newer.tip[1]).toBe('The station runs 1.2.0-test.1, newer than this app: update the app to use everything there.');
    const major = versionLabel('1.1.0', '2.0.0');
    expect(major.station).toBe('2.0.0');
    expect(major.tip[1]).toContain('a different major version');
  });

  it('says a version it cannot compare in the tooltip only', () => {
    expect(versionLabel('1.1.0', 'dev')).toEqual({ app: '1.1.0', station: null, stationShort: null, tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs dev.'] });
  });
});

describe('the frame', () => {
  beforeEach(() => {
    studio.view = null;
    studio.stationVersion = null;
  });

  it('says the app’s version in its spare room, between the tabs and who is signed in, without Tauri', () => {
    const f = frame();
    expect(words(f.label)).toBe(`WeaveSignal v${APP_VERSION}`); // the name or the v is drawn, by the room there is
    expect(f.label).toMatch(/<span class="name [^"]*">WeaveSignal(&nbsp;|\u00a0)<\/span><span class="v [^"]*">v<\/span>/);
    expect(f.frame.indexOf('<span class="spacer')).toBeGreaterThan(f.frame.indexOf('</nav>'));
    expect(f.frame.indexOf('data-testid="app-version"')).toBeGreaterThan(f.frame.indexOf('<span class="spacer'));
    expect(f.frame.indexOf('data-testid="app-version"')).toBeLessThan(f.frame.indexOf('data-panel="studio.account"'));
  });

  it('is words with a tooltip, not a button: a tab stop, the tooltip named, no state chip and no status colour', () => {
    const f = frame();
    expect(f.label).toMatch(/<span[^>]*class="ver st-trigger[^"]*"[^>]*data-testid="app-version"[^>]*aria-describedby="ver-tip"[^>]*tabindex="0"/);
    expect(f.label).not.toContain('<button');
    expect(f.label).not.toMatch(/class="state|hollow-|fill-|g-warning|g-degraded/);
    expect(f.tip).toContain('role="tooltip"');
    expect(words(f.tip)).toBe(`This studio app is WeaveSignal ${APP_VERSION}.`);
  });

  it('adds the station’s version only when it differs, and the tooltip says both', () => {
    studio.stationVersion = APP_VERSION;
    let f = frame();
    expect(f.label).not.toContain('class="stn');
    expect(f.label).not.toMatch(/class="ver [^"]*differs/);
    expect(words(f.tip)).toBe(`This studio app is WeaveSignal ${APP_VERSION}. The station runs the same version.`);

    studio.stationVersion = '0.9.3';
    f = frame();
    expect(f.label).toMatch(/class="ver [^"]*differs/);
    // Each form the frame may draw is in the page; the styles show the one there is room for.
    expect(words(f.label)).toBe(`WeaveSignal v${APP_VERSION} · station 0.9.3 ≠ 0.9.3`);
    expect(f.label).toMatch(/<span class="stn [^"]*"> · station 0\.9\.3<\/span><span class="neq [^"]*"> ≠ 0\.9\.3<\/span>/);
    expect(words(f.tip)).toContain('The station runs 0.9.3,');
  });

  it('is in the frame’s description in the panel registry', () => {
    expect(PANELS['studio.frame']).toContain("the app's version");
  });
});

describe('reading the station’s version again', () => {
  it('waits for the first reading, then asks again when the link comes back after a while', () => {
    expect(rereadNow(0, 1_000_000)).toBe(false);
    expect(rereadNow(1_000_000, 1_000_000 + REREAD_MS - 1)).toBe(false);
    expect(rereadNow(1_000_000, 1_000_000 + REREAD_MS)).toBe(true);
  });
});
