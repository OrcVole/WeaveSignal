// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Settings > Chat pictures (ADR-0259, ADR-0257): the switch for uploaded pictures, read from and
// written to the chat's own settings, and the words the panel shows. The station stays the
// authority; nothing here decides who may do what.

import { ApiError, headers, netFetch } from './api.ts';
import { chatBase } from './chat.svelte.ts';

/** The chat's settings document as the station returns it; only the fields this panel touches
 *  are named, and the rest is carried through a change untouched, so a switch never clobbers the
 *  terms or the word lists. */
export interface ChatSettings {
  avatar_uploads?: boolean | null;
  /** Pictures and documents in the chat (ADR-0262): null until the switch is used. */
  attachments?: boolean | null;
  [field: string]: unknown;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await netFetch(`${chatBase()}${path}`, {
    method,
    headers: headers(body !== undefined),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(res.status, data.error ?? `${res.status} ${res.statusText}`);
  return data as T;
}

export const getSettings = () => call<ChatSettings>('GET', '/api/v1/chat/admin/settings');
export const putSettings = (s: ChatSettings) => call<ChatSettings>('PUT', '/api/v1/chat/admin/settings', s);

/** Whether uploads are on now, and whether the switch has been used (else the install setting
 *  decides). */
export interface UploadsView {
  on: boolean;
  /** The switch has been used: the station's choice, not the install setting. */
  chosen: boolean;
}

/** The view from the settings and the public info's reading of the effective value. */
export function uploadsView(settings: Pick<ChatSettings, 'avatar_uploads'>, effective: boolean): UploadsView {
  const chosen = typeof settings.avatar_uploads === 'boolean';
  return { on: chosen ? (settings.avatar_uploads as boolean) : effective, chosen };
}

export async function readUploads(): Promise<UploadsView> {
  const [settings, info] = await Promise.all([getSettings(), call<{ avatars?: { uploads: boolean } }>('GET', '/api/v1/chat/info')]);
  return uploadsView(settings, info.avatars?.uploads ?? false);
}

/** Turn uploads on or off at once: read the document, change one field, put it back. */
export async function setUploads(on: boolean): Promise<UploadsView> {
  const current = await getSettings();
  const saved = await putSettings({ ...current, avatar_uploads: on });
  return uploadsView(saved, on);
}

// ---- pictures and documents posted in the chat (ADR-0262) ----

/** The view from the settings and the public info's reading of the effective value. */
export function attachmentsView(settings: Pick<ChatSettings, 'attachments'>, effective: boolean): UploadsView {
  const chosen = typeof settings.attachments === 'boolean';
  return { on: chosen ? (settings.attachments as boolean) : effective, chosen };
}

export async function readAttachments(): Promise<UploadsView> {
  const [settings, info] = await Promise.all([getSettings(), call<{ attachments?: { uploads: boolean } }>('GET', '/api/v1/chat/info')]);
  return attachmentsView(settings, info.attachments?.uploads ?? false);
}

/** Turn pictures and documents on or off at once: read the document, change one field, put it back. */
export async function setAttachments(on: boolean): Promise<UploadsView> {
  const current = await getSettings();
  const saved = await putSettings({ ...current, attachments: on });
  return attachmentsView(saved, on);
}

/** One sentence on where the switch stands. */
export function attachmentsWords(v: UploadsView): string {
  if (v.on) {
    return v.chosen
      ? 'Trusted chatters can post a picture or a document. Each one waits for staff or a moderator before anyone else sees it.'
      : 'Trusted chatters can post a picture or a document, as the station was installed. Each one waits for staff or a moderator before anyone else sees it.';
  }
  return v.chosen
    ? 'Off. The chat takes words only; pictures and documents already posted stay as they are.'
    : 'Off, as the station was installed. The chat takes words only.';
}

// ---- pictures kept out of view, and the evidence (ADR-0257) ----

/** A picture the station keeps: refused or removed and withheld (`GET /api/v1/chat/mod/kept-pictures`). */
export interface KeptPicture {
  id: string;
  account: { id: string; name: string };
  reason: string | null;
  reason_words: string | null;
  uploaded_ms: number;
  decided_ms: number | null;
  decided_by: string | null;
  /** Evidence only: the earliest the station deletes it by itself. */
  keep_until_ms: number | null;
  /** Kept as evidence of suspected child sexual abuse material: only the named person sees it. */
  evidence: boolean;
  has_original: boolean;
  /** An avatar, or a picture or document posted in the chat (ADR-0262); absent before 1.0.1. */
  kind?: 'avatar' | 'picture' | 'document';
  /** A posted file's own name, shown as text. */
  file_name?: string | null;
}

/** What a kept upload is, in a few words. */
export function keptKind(p: Pick<KeptPicture, 'kind' | 'file_name'>): string {
  if (p.kind === 'document') return `Document${p.file_name ? ` “${p.file_name}”` : ''}`;
  if (p.kind === 'picture') return `Picture posted in the chat${p.file_name ? ` (“${p.file_name}”)` : ''}`;
  return 'Avatar picture';
}

export interface Kept {
  pictures: KeptPicture[];
  evidence_viewer: { id: string; name: string } | null;
  you_are_the_evidence_viewer: boolean;
}

export const kept = () => call<Kept>('GET', '/api/v1/chat/mod/kept-pictures');

/** Delete a kept picture from the server at once, with why. One kept as evidence needs `confirm`,
 *  after the words below; the station checks it again. */
export const deleteKept = (avatar: string, why: string, confirm: boolean) =>
  call<{ done: string }>('POST', '/api/v1/chat/mod', { op: 'avatar_delete', avatar, why, confirm_evidence: confirm || undefined });

/** Name who sees the evidence, or `null` for the first admin again. */
export async function setEvidenceViewer(id: string | null): Promise<ChatSettings> {
  const current = await getSettings();
  return putSettings({ ...current, evidence_viewer: id });
}

/** What deleting a picture kept as evidence says before it asks to be confirmed (the station's
 *  own sentence, `EVIDENCE_DELETE_WORDS`, is shown if it refuses). */
export const EVIDENCE_DELETE_WORDS =
  'This picture is kept as evidence of suspected child sexual abuse material. The law asks you to report it and then keep it, for at least a year. Delete it only if you have decided that is right for the station; the deletion is recorded.';

/** A picture's bytes, for the one person who may see it: an object URL to show and then revoke. */
export async function evidencePictureUrl(id: string): Promise<string> {
  const res = await netFetch(`${chatBase()}/api/v1/chat/evidence/${encodeURIComponent(id)}/picture`, { headers: headers(false) });
  if (!res.ok) throw new ApiError(res.status, (await res.json().catch(() => ({}))).error ?? `${res.status} ${res.statusText}`);
  return URL.createObjectURL(await res.blob());
}

/** The evidence for a report: the station's ZIP as a file to save. */
export async function evidenceExport(id: string): Promise<{ name: string; blob: Blob }> {
  const res = await netFetch(`${chatBase()}/api/v1/chat/evidence/${encodeURIComponent(id)}/export`, { headers: headers(false) });
  if (!res.ok) throw new ApiError(res.status, (await res.json().catch(() => ({}))).error ?? `${res.status} ${res.statusText}`);
  return { name: `evidence-${id}.zip`, blob: await res.blob() };
}

/** `12 Nov 2026`, in the reader's zone. */
export const dayWords = (ms: number): string => new Date(ms).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });

/** One line on why a picture is kept and for how long. */
export function keptWords(p: KeptPicture): string {
  const why = p.reason_words ?? 'Kept under an evidence hold';
  const by = p.decided_by ? `, by ${p.decided_by}` : '';
  const when = p.decided_ms ? ` on ${dayWords(p.decided_ms)}` : '';
  const until = p.evidence && p.keep_until_ms ? ` Kept until at least ${dayWords(p.keep_until_ms)}.` : p.evidence ? '' : ' Kept until its evidence hold is released.';
  return `${why}${by}${when}.${until}`;
}

/** Who sees the evidence, in a sentence. */
export function viewerWords(v: Kept['evidence_viewer'], you: boolean): string {
  if (!v) return 'The station has no admin to see a picture kept as evidence: add one.';
  return you ? 'You are the one person who can see pictures kept as evidence.' : `${v.name} is the one person who can see pictures kept as evidence.`;
}

/** One sentence on where the switch stands. */
export function uploadsWords(v: UploadsView): string {
  if (v.on) {
    return v.chosen
      ? 'Chatters can upload a picture of their own. Each one waits for a moderator before anyone else sees it.'
      : 'Chatters can upload a picture of their own, as the station was installed. Each one waits for a moderator before anyone else sees it.';
  }
  return v.chosen
    ? 'Uploads are off. Chatters choose from the avatars that come with the station; pictures already waiting or showing stay as they are.'
    : 'Uploads are off, as the station was installed. Chatters choose from the avatars that come with the station.';
}
