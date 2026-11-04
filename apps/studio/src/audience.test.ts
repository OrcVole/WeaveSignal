// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The audience figure and the CDN warning (ADR-0046, ADR-0017): above about 500 listeners with
// video on and no edges or CDN, desk B's `desk.audience` says so.
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { VIDEO_VIEWERS_WITHOUT_CDN, cdnWarning } from './lib/audience.ts';
import type { AudienceView, PictureView, View } from './lib/api.ts';
import DeskB from './desk/DeskB.svelte';
import { studio } from './lib/studio.svelte.ts';

const pic: PictureView = { mode: { mode: 'auto' }, shot: 'slate', cameras: [], hls: true, room: true };

/** The least of a view that the warning and the audience panel read. */
function view(audience: AudienceView | undefined, picture: PictureView | null): View {
  return {
    station: 'Kettle FM',
    now_ms: Date.UTC(2026, 10, 2, 12),
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 0 }, picture },
    seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
    sources: [],
    callers: [],
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
    audience,
  };
}

describe('the CDN warning', () => {
  it('warns above 500 with video on and no edges, with the count in words', () => {
    const w = cdnWarning(view({ listeners: 612, edges: false }, pic));
    expect(w?.title).toBe('Video needs a CDN above about 500 viewers');
    expect(w?.message).toContain('612 people are connected');
    expect(w?.message).toContain('no CDN or edges');
    expect(w?.message).toContain('switch video off');
  });

  it('is quiet at 500 and below (the decision says more than about 500)', () => {
    expect(VIDEO_VIEWERS_WITHOUT_CDN).toBe(500);
    expect(cdnWarning(view({ listeners: 500, edges: false }, pic))).toBeNull();
    expect(cdnWarning(view({ listeners: 501, edges: false }, pic))).not.toBeNull();
    expect(cdnWarning(view({ listeners: 0, edges: false }, pic))).toBeNull();
  });

  it('is quiet with video off, with edges or a CDN, or when nobody counted', () => {
    expect(cdnWarning(view({ listeners: 2000, edges: false }, null))).toBeNull();
    expect(cdnWarning(view({ listeners: 2000, edges: true }, pic))).toBeNull();
    expect(cdnWarning(view({ listeners: null, edges: false }, pic))).toBeNull();
    expect(cdnWarning(view(undefined, pic))).toBeNull(); // a control plane too old to report it
    expect(cdnWarning(null)).toBeNull();
  });
});

describe('desk.audience', () => {
  it('shows the count, and the warning inside the panel when it applies', () => {
    studio.view = view({ listeners: 612, edges: false }, pic);
    const { body } = render(DeskB);
    expect(body).toContain('612');
    expect(body).toContain('listeners now');
    expect(body).toContain('data-warning="cdn"');
    expect(body).toContain('Video needs a CDN above about 500 viewers');
    expect(body.indexOf('data-warning="cdn"')).toBeGreaterThan(body.indexOf('data-panel="desk.audience"'));
    expect(body.indexOf('data-warning="cdn"')).toBeLessThan(body.indexOf('data-panel="desk.picture"'));
  });

  it('says not counted yet before there is a figure, and shows no warning', () => {
    studio.view = view({ listeners: null, edges: false }, pic);
    const { body } = render(DeskB);
    expect(body).toContain('not counted yet');
    expect(body).not.toContain('data-warning="cdn"');
  });

  it('shows the count without the warning when edges carry the picture', () => {
    studio.view = view({ listeners: 1800, edges: true }, pic);
    const { body } = render(DeskB);
    expect(body).toContain('1,800');
    expect(body).not.toContain('data-warning="cdn"');
  });
});
