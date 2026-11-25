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
import { REREAD_MS, STANDING_WORD, rereadNow, standing, versionLabel } from './lib/version.ts';

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
    expect(versionLabel('1.1.0', null)).toEqual({ app: '1.1.0', station: null, stationShort: null, standing: 'unknown', tip: ['This studio app is WeaveSignal 1.1.0.'] });
    expect(versionLabel('1.1.0', '1.1.0')).toEqual({ app: '1.1.0', station: null, stationShort: null, standing: 'current', tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs the same version.', 'This app is up to date.'] });
    // Only the numbers count: a test build of the same release is not a difference, and the
    // tooltip still says exactly what the station runs.
    expect(versionLabel('1.1.0', '1.1.0-test.4')).toEqual({ app: '1.1.0', station: null, stationShort: null, standing: 'current', tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs 1.1.0-test.4, the same release.', 'This app is up to date.'] });
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
    expect(versionLabel('1.1.0', 'dev')).toEqual({ app: '1.1.0', station: null, stationShort: null, standing: 'unknown', tip: ['This studio app is WeaveSignal 1.1.0.', 'The station runs dev.'] });
  });
});

describe('where the app stands (1.3.2: the ink of its version)', () => {
  it('is up to date while the station runs the same release and no update is offered', () => {
    expect(standing('1.3.2', '1.3.2')).toBe('current');
    expect(standing('1.3.2', '1.3.2-test.1')).toBe('current');
  });

  it('is behind when the station is newer, by a patch, a minor or a major version', () => {
    expect(standing('1.3.1', '1.3.2')).toBe('behind');
    expect(standing('1.3.1', '1.4.0')).toBe('behind');
    expect(standing('1.3.1', '2.0.0')).toBe('behind');
  });

  it('is behind when the update feed offers a newer app, whatever the station runs', () => {
    expect(standing('1.3.1', '1.3.1', '1.3.2')).toBe('behind');
    expect(standing('1.3.1', null, '1.3.2')).toBe('behind');
    expect(standing('1.3.1', '1.3.0', '1.3.2')).toBe('behind');
  });

  it('is ahead, with nothing to do to the app, when the station is older', () => {
    expect(standing('1.3.2', '1.3.1')).toBe('ahead');
    expect(standing('2.0.0', '1.9.9')).toBe('ahead');
  });

  it('is not known until the station has said a version this app can compare', () => {
    expect(standing('1.3.2', null)).toBe('unknown');
    expect(standing('1.3.2', 'dev')).toBe('unknown');
  });

  it('is said in words too: a word for the label, and a sentence closing the tooltip', () => {
    expect(STANDING_WORD).toEqual({ current: 'up to date', behind: 'behind', ahead: '', unknown: '' });
    expect(versionLabel('1.3.1', '1.3.2').tip.at(-1)).toBe('This app is behind the station.');
    expect(versionLabel('1.3.1', '1.3.1', '1.3.2').tip.at(-1)).toBe('Behind: WeaveSignal 1.3.2 is available. Settings › Updates installs it.');
    expect(versionLabel('1.3.2', '1.3.2').tip.at(-1)).toBe('This app is up to date.');
    // Ahead and unknown take no ink, so the tooltip adds no sentence about it.
    expect(versionLabel('1.3.2', '1.3.1').tip).toHaveLength(2);
    expect(versionLabel('1.3.2', null).tip).toHaveLength(1);
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

  it('is words with a tooltip, not a button: a tab stop, the tooltip named, no state chip; no ink or glyph until it knows where it stands', () => {
    const f = frame();
    expect(f.label).toMatch(/<span[^>]*class="ver st-trigger[^"]*"[^>]*data-standing="unknown"[^>]*data-testid="app-version"[^>]*aria-describedby="ver-tip"[^>]*tabindex="0"/);
    expect(f.label).not.toContain('<button');
    expect(f.label).not.toMatch(/class="state|hollow-|fill-|g-warning|g-degraded|<svg/);
    expect(f.label).not.toMatch(/class="ver [^"]*cue/);
    expect(f.tip).toContain('role="tooltip"');
    expect(words(f.tip)).toBe(`This studio app is WeaveSignal ${APP_VERSION}.`);
  });

  it('says up to date three ways, never by its ink alone: the tick, a word in the label, a sentence in the tooltip', () => {
    studio.stationVersion = APP_VERSION;
    const f = frame();
    expect(f.label).toMatch(/class="ver [^"]*cue[^"]*"[^>]*data-standing="current"/);
    expect(f.label).toMatch(/<svg class="vg [^"]*" aria-hidden="true"><use href="#g-check"/);
    expect(f.label).toMatch(/<span class="sr-only">, up to date<\/span>/);
    expect(words(f.label)).toBe(`WeaveSignal v${APP_VERSION}, up to date`);
    expect(words(f.tip)).toContain('This app is up to date.');
  });

  it('says behind three ways when the station is newer: the arrow, the word, the sentence', () => {
    studio.stationVersion = '99.0.0';
    const f = frame();
    expect(f.label).toMatch(/class="ver [^"]*"[^>]*data-standing="behind"/);
    expect(f.label).toMatch(/<use href="#g-behind"/);
    expect(f.label).toMatch(/<span class="sr-only">, behind<\/span>/);
    expect(words(f.tip)).toContain('This app is behind the station.');
  });

  it('takes neither ink nor glyph while the app is ahead of the station: the words say the difference', () => {
    studio.stationVersion = '0.9.3';
    const f = frame();
    expect(f.label).toMatch(/data-standing="ahead"/);
    expect(f.label).not.toContain('<svg');
    expect(f.label).not.toContain('sr-only');
  });

  it('adds the station’s version only when it differs, and the tooltip says both', () => {
    studio.stationVersion = APP_VERSION;
    let f = frame();
    expect(f.label).not.toContain('class="stn');
    expect(f.label).not.toMatch(/class="ver [^"]*differs/);
    expect(words(f.tip)).toBe(`This studio app is WeaveSignal ${APP_VERSION}. The station runs the same version. This app is up to date.`);

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
