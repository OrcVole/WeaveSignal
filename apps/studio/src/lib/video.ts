// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The video path's side routes (ADR-0010): the cameras' thumbnails and the slate's stills,
// the station's and each show's (ADR-0200).

import { ApiError, base, headers, netFetch } from './api.ts';

/** A still as the control plane reports it. */
export interface StillInfo {
  present: boolean;
  width: number;
  height: number;
  bytes: number;
  /** When it was set, ms since the Unix epoch. */
  set_at_ms: number;
}

/** No still. */
export const NO_STILL: StillInfo = { present: false, width: 0, height: 0, bytes: 0, set_at_ms: 0 };

/** The route of a still: the station's without a show, else the show's own (ADR-0200). */
export function stillRoute(show?: string): string {
  return show ? `/api/v1/shows/${encodeURIComponent(show)}/still` : '/api/v1/video/still';
}

/** A small picture of each camera the desk sees, by identity, as `data:` URLs (the page's
 *  content policy allows `data:` images, and an `<img>` cannot carry the bearer). */
export async function cameraThumbs(): Promise<Record<string, string>> {
  const res = await netFetch(`${base()}/api/v1/video/thumbs`, { headers: headers(false) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return thumbUrls(data.thumbs);
}

/** The control plane's base64 JPEGs as `data:` URLs; anything else is dropped. */
export function thumbUrls(thumbs: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (!thumbs || typeof thumbs !== 'object') return out;
  for (const [id, b64] of Object.entries(thumbs as Record<string, unknown>)) {
    if (typeof b64 === 'string' && /^[A-Za-z0-9+/]+=*$/.test(b64)) out[id] = `data:image/jpeg;base64,${b64}`;
  }
  return out;
}

/** The station's slate still, or a show's. */
export async function stillInfo(show?: string): Promise<StillInfo> {
  const res = await netFetch(`${base()}${stillRoute(show)}`, { headers: headers(false) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as StillInfo;
}

/** Make this picture (PNG or JPEG) the station's still, or a show's; the station's sentence
 *  if it is refused. */
export async function putStill(file: Blob, show?: string): Promise<StillInfo> {
  const h = headers(false) as Record<string, string>;
  h['Content-Type'] = file.type || 'application/octet-stream';
  const res = await netFetch(`${base()}${stillRoute(show)}`, { method: 'PUT', headers: h, body: file });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as StillInfo;
}

/** Take the still away: the slate is words again (the station's), or shows the station's
 *  picture (a show's). */
export async function removeStill(show?: string): Promise<void> {
  const res = await netFetch(`${base()}${stillRoute(show)}`, { method: 'DELETE', headers: headers(false) });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  }
}

/** The still as the desk shows it, a `data:` URL for the studio's preview (the card preview's
 *  way: an `<img>` cannot carry the bearer); null when there is none. */
export async function stillImage(show?: string): Promise<string | null> {
  const res = await netFetch(`${base()}${stillRoute(show)}/image`, { headers: headers(false) });
  if (res.status === 404) return null;
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  }
  return dataUrl('image/png', new Uint8Array(await res.arrayBuffer()));
}

/** Bytes as a `data:` URL. */
export function dataUrl(type: string, bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return `data:${type};base64,${btoa(bin)}`;
}

/** What to tell a person when a still is refused: the station's sentence, except the size
 *  limit, which comes back as a bare 413 without one. */
export function stillProblem(err: unknown): string {
  if (err instanceof ApiError && err.status === 413) return 'The picture is too large; the station takes up to 24 MB unless set otherwise.';
  return err instanceof Error ? err.message : String(err);
}
