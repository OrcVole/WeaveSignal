// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The call-in lobby's rules: a name, one line on what the caller wants to say, and their
// consent to be broadcast and recorded. The consent is required; without it there is no call
// request at all, and the control plane refuses one that arrives anyway (the legal item the
// screener design calls out, ADR-0006's flow). Pure, so the rules are tested.

import type { CallInRequest } from './api.ts';

export interface LobbyForm {
  name: string;
  topic: string;
  consent: boolean;
}

export type LobbyErrors = Partial<Record<keyof LobbyForm, string>>;

export interface Limits {
  name_max: number;
  topic_max: number;
}

/** What is wrong with the form, field by field, in the words shown beside each field. */
export function check(f: LobbyForm, lim: Limits): LobbyErrors {
  const e: LobbyErrors = {};
  const name = f.name.trim();
  const topic = f.topic.trim();
  if (!name) e.name = "Add your name, or the name you'd like said on air.";
  else if ([...name].length > lim.name_max) e.name = `Your name is at most ${lim.name_max} characters.`;
  if (!topic) e.topic = "Say in a few words what you'd like to talk about.";
  else if ([...topic].length > lim.topic_max) e.topic = `Keep it to ${lim.topic_max} characters; the producer will ask for more.`;
  if (!f.consent) e.consent = 'Tick the box to agree to your call being broadcast and recorded. You cannot call without it.';
  return e;
}

/** The request to send, or null while anything is wrong: no consent, no request. */
export function request(f: LobbyForm, lim: Limits, version: string): CallInRequest | null {
  if (Object.keys(check(f, lim)).length) return null;
  return { name: f.name.trim(), topic: f.topic.trim(), consent: { accepted: true, version } };
}

/** The field to move focus to after a failed submit: the first in reading order. */
export function firstError(e: LobbyErrors): keyof LobbyForm | null {
  return (['name', 'topic', 'consent'] as const).find((k) => e[k]) ?? null;
}
