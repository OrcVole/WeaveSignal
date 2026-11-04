// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's mode in the studio view (ADR-0045, ADR-0183): the data the AUTO lamp reads. The
// lamp's own states are tested in rules.test.ts and desk.test.ts.
import { describe, expect, it } from 'vitest';
import type { StationMode } from '../../../packages/ui/states.ts';
import type { View } from './lib/api.ts';
import { studio, takeView } from './lib/studio.svelte.ts';

/** A view as the control plane sends it, with `mode` as given (left out when undefined). */
function view(mode: StationMode | null | undefined): View {
  const v: View = {
    station: 'Kettle FM',
    now_ms: Date.UTC(2026, 10, 2, 12),
    show: { title: 'The Morning Kettle', rundown: [] },
    desk: { reachable: true, error: null, playout: { connected: true, for_ms: 0 } },
    seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
    sources: [],
    callers: [],
    desk_taking_callers: true,
    chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
    notices: [],
    carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
    listener_delay: { ms: 15_000, measured: false },
  };
  if (mode !== undefined) v.mode = mode;
  // Through JSON, as the event stream delivers it.
  return JSON.parse(JSON.stringify(v)) as View;
}

describe("the station's mode in the view", () => {
  it('arrives as live or auto, null until read, and absent from an older control plane', () => {
    takeView(view('auto'));
    expect(studio.view?.mode).toBe('auto');
    takeView(view('live'));
    expect(studio.view?.mode).toBe('live');
    takeView(view(null));
    expect(studio.view?.mode).toBeNull();
    takeView(view(undefined));
    expect(studio.view?.mode).toBeUndefined();
  });
});
