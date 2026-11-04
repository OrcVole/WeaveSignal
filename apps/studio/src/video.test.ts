// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The camera strip and the slate's picture on desk B's `desk.picture` (ADR-0010), and a show's
// own picture in the show editor (ADR-0200): the routes, the shared control, the field.
import { render as ssr } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import DeskB from './desk/DeskB.svelte';
import Cameras from './desk/Cameras.svelte';
import SlateStill from './desk/SlateStill.svelte';
import ShowStill from './schedule/ShowStill.svelte';
import { StillControl } from './lib/still.svelte.ts';
import { ApiError } from './lib/api.ts';
import { dataUrl, putStill, removeStill, stillImage, stillInfo, stillProblem, stillRoute, thumbUrls } from './lib/video.ts';
import type { PictureView, View } from './lib/api.ts';
import { studio } from './lib/studio.svelte.ts';

/** The server render without its hydration comments. */
const render: typeof ssr = ((c: never, o: never) => {
  const r = ssr(c, o);
  return { ...r, body: r.body.replace(/<!--.*?-->/g, '') };
}) as typeof ssr;

describe('the camera thumbnails the desk makes', () => {
  it('become data URLs, and nothing else gets through', () => {
    expect(thumbUrls({ eilidh: '/9j/4AAQSkZJRg==', bad: 'x" onerror="y', n: 4 })).toEqual({
      eilidh: 'data:image/jpeg;base64,/9j/4AAQSkZJRg==',
    });
    expect(thumbUrls(null)).toEqual({});
    expect(thumbUrls('nope')).toEqual({});
  });
});

describe('the camera strip', () => {
  it('names each camera and marks the one on air, with a blank until its picture comes', () => {
    const { body } = render(Cameras, { props: { cameras: ['eilidh', 'niall'], onair: 'niall', label: (id: string) => id.toUpperCase() } });
    expect(body).toContain('aria-label="Cameras"');
    expect(body).toContain('EILIDH');
    expect(body.match(/on air/g)).toHaveLength(1);
    expect(body).toMatch(/NIALL<\/span><span class="air">on air/);
    expect(body).toContain('No picture yet for EILIDH');
  });

  it('draws nothing with no cameras', () => {
    const { body } = render(Cameras, { props: { cameras: [], label: (id: string) => id } });
    expect(body).not.toContain('Cameras');
  });
});

describe('the slate picture control', () => {
  it('says none, or set, and offers Remove only when there is one', () => {
    const none = render(SlateStill, { props: { on: false } }).body;
    expect(none).toContain('Slate picture: none');
    expect(none).not.toContain('Remove');
    expect(none).toContain('accept="image/png,image/jpeg"');
    const set = render(SlateStill, { props: { on: true } }).body;
    expect(set).toContain('Slate picture: set');
    expect(set).toContain('Remove');
  });
});

describe('desk.picture', () => {
  it('carries the strip and the slate control inside its own panel', () => {
    const pic: PictureView = { mode: { mode: 'auto' }, shot: 'person', identity: 'eilidh', cameras: ['eilidh'], hls: true, room: true, still: true };
    studio.view = {
      station: 'Kettle FM',
      now_ms: Date.UTC(2026, 10, 2, 12),
      show: { title: 'The Morning Kettle', rundown: [] },
      desk: { reachable: true, error: null, playout: { connected: true, for_ms: 0 }, picture: pic },
      seats: { presenter: null, screener: null, presenter_name: '', screener_name: '' },
      sources: [],
      callers: [],
      desk_taking_callers: true,
      chat: { slow: false, hold_new: false, links_trusted: true, accounts_only: false, min_age_24h: false, influx: false, influx_since_ms: null },
      notices: [],
      carts: { version: 0, desk: true, desk_error: null, sounding: [], ducked: false, mic_open: false, missing: [], importing: null },
      listener_delay: { ms: 15_000, measured: false },
    } as View;
    const { body } = render(DeskB);
    const from = body.indexOf('data-panel="desk.picture"');
    const to = body.indexOf('data-panel="desk.carts"');
    const panel = body.slice(from, to);
    expect(panel).toContain('aria-label="Cameras"');
    expect(panel).toContain('Slate picture: set');
  });
});

/** A control plane answering one request: its status and body (JSON, or bytes). */
function answer(status: number, body: unknown = {}) {
  const calls: { url: string; method: string; type: string | null }[] = [];
  const fetch = vi.fn(async (url: string, init: RequestInit = {}) => {
    calls.push({ url, method: init.method ?? 'GET', type: new Headers(init.headers).get('content-type') });
    if (status === 204) return new Response(null, { status });
    const bytes = body instanceof Uint8Array ? body : new TextEncoder().encode(JSON.stringify(body));
    return new Response(bytes.slice().buffer, { status, headers: { 'content-type': body instanceof Uint8Array ? 'image/png' : 'application/json' } });
  });
  vi.stubGlobal('fetch', fetch);
  return calls;
}

afterEach(() => vi.unstubAllGlobals());

describe("the stills' routes", () => {
  it("are the station's without a show and the show's own with one (ADR-0200)", () => {
    expect(stillRoute()).toBe('/api/v1/video/still');
    expect(stillRoute('the-late-line')).toBe('/api/v1/shows/the-late-line/still');
    expect(stillRoute('a b')).toBe('/api/v1/shows/a%20b/still');
  });

  it('ask, send and take away on the right route with the picture as the body', async () => {
    const calls = answer(201, { present: true, width: 1280, height: 720, bytes: 4096, set_at_ms: 1 });
    expect((await stillInfo('late-line')).width).toBe(1280);
    const sent = await putStill(new Blob([new Uint8Array([137, 80, 78, 71])], { type: 'image/png' }), 'late-line');
    expect(sent.present).toBe(true);
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual(['GET /api/v1/shows/late-line/still', 'PUT /api/v1/shows/late-line/still']);
    expect(calls[1].type).toBe('image/png');
    answer(204);
    await expect(removeStill('late-line')).resolves.toBeUndefined();
    answer(204);
    await expect(removeStill()).resolves.toBeUndefined();
  });

  it("carry the station's sentence when it refuses a picture", async () => {
    answer(422, { error: 'the picture could not be read: not a PNG or JPEG' });
    await expect(putStill(new Blob(['x'], { type: 'text/plain' }), 'late-line')).rejects.toMatchObject({ status: 422, message: 'the picture could not be read: not a PNG or JPEG' });
    answer(404, { error: 'no show `gone`' });
    await expect(stillInfo('gone')).rejects.toMatchObject({ status: 404, message: 'no show `gone`' });
  });

  it('say the size limit in words, since a bare 413 has none', () => {
    expect(stillProblem(new ApiError(413, '413 '))).toMatch(/too large.*24 MB/);
    expect(stillProblem(new ApiError(422, 'not a picture'))).toBe('not a picture');
    expect(stillProblem('odd')).toBe('odd');
  });

  it('fetch the picture as a data URL for the preview, or null when there is none', async () => {
    const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
    const calls = answer(200, png);
    expect(await stillImage('late-line')).toBe(dataUrl('image/png', png));
    expect(calls[0].url).toBe('/api/v1/shows/late-line/still/image');
    answer(404, { error: 'the show has no still' });
    expect(await stillImage('late-line')).toBeNull();
    expect(dataUrl('image/png', png)).toBe('data:image/png;base64,iVBORw0KGgo=');
  });
});

describe('the still control shared by the desk and the show editor', () => {
  it('looks, sends, keeps the refusal, and takes away', async () => {
    const info = { present: true, width: 640, height: 360, bytes: 2048, set_at_ms: 5 };
    answer(200, info);
    const c = new StillControl('late-line');
    expect(c.info).toBeNull();
    await c.load();
    expect(c.info).toEqual(info);
    expect(c.busy).toBe(false);
    answer(422, { error: 'not a picture' });
    await c.choose(new Blob(['x']));
    expect(c.problem).toBe('not a picture');
    expect(c.info).toEqual(info);
    answer(204);
    await c.remove();
    expect(c.problem).toBe('');
    expect(c.info?.present).toBe(false);
  });

  it('with a preview, fetches the picture after a look and after a send', async () => {
    const png = new Uint8Array([1, 2, 3]);
    const fetch = vi.fn(async (url: string) =>
      url.endsWith('/image')
        ? new Response(png, { status: 200, headers: { 'content-type': 'image/png' } })
        : new Response(JSON.stringify({ present: true, width: 1, height: 1, bytes: 3, set_at_ms: 1 }), { status: 201, headers: { 'content-type': 'application/json' } }),
    );
    vi.stubGlobal('fetch', fetch);
    const c = new StillControl('late-line', true);
    await c.load();
    expect(c.image).toBe(dataUrl('image/png', png));
    await c.choose(new Blob([png], { type: 'image/png' }));
    expect(fetch.mock.calls.map((a) => a[0])).toEqual([
      '/api/v1/shows/late-line/still',
      '/api/v1/shows/late-line/still/image',
      '/api/v1/shows/late-line/still',
      '/api/v1/shows/late-line/still/image',
    ]);
  });

  it('a look that fails leaves nothing set and says nothing', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('offline'); }));
    const c = new StillControl();
    await c.load();
    expect(c.info).toBeNull();
    expect(c.problem).toBe('');
    expect(c.busy).toBe(false);
  });
});

describe("the show editor's slate picture field (calendar.show-still)", () => {
  it('is its own panel, says in one line what the picture does, and offers Choose… before a picture is known', () => {
    const { body } = render(ShowStill, { props: { slug: 'late-line' } });
    expect(body).toContain('data-panel="calendar.show-still"');
    expect(body).toContain('data-panel-title');
    expect(body).toContain("The slate shows it while this show is on air; otherwise it shows the station's picture.");
    expect(body).toContain('>Choose…<');
    expect(body).not.toContain('Remove');
    expect(body).toContain('accept="image/png,image/jpeg"');
    expect(body).toContain('aria-live="polite"');
    expect(body).toContain('No picture of its own.');
  });
});
