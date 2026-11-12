// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Kettle FM's people, as the design system's section 9 names them (ADR-0071: the Tartan
// National Names list, each person keeping one name everywhere). The same person has the same
// identity in every scenario, so a recorded clip serves them all.

import type { Cast, CallerSeed } from './scenario.ts';

export const EILIDH: Cast = { identity: 'eilidh', name: 'Eilidh Fraser-MacLeod', role: 'presenter' };
export const GREGOR: Cast = { identity: 'gregor', name: 'Gregor Stewart', role: 'guest' };
export const SHONA: Cast = { identity: 'shona', name: 'Shona Campbell', role: 'screener' };
/** The remote DJ, standing by from home for Steeped. */
export const FINLAY: Cast = { identity: 'finlay', name: 'Finlay Grant', role: 'dj' };

/** The callers, with what they type on the player page. */
export const NIALL: CallerSeed = { name: 'Niall Anderson', topic: 'The bus lanes on Leith Walk' };
export const JESSIE: CallerSeed = { name: 'Jessie Brown', topic: 'No night bus from the Infirmary' };
export const CALUM: CallerSeed = { name: 'Calum Robertson', topic: 'Parking at Easter Road on match days' };
/** The caller on slot 4 (section 9). */
export const DOUGAL: CallerSeed = { name: 'Dougal Thomson', topic: 'The bins in Portobello' };
/** The caller whose link drops (section 9). */
export const IONA: CallerSeed = { name: 'Iona MacIntosh', topic: 'The ferry fares, from Derry' };
/** The dropped caller, Musselburgh (section 9). */
export const FLORA: CallerSeed = { name: 'Flora Armstrong', topic: 'Just to say hello' };
/** The caller on air, Portobello (section 9). */
export const ALASTAIR: CallerSeed = { name: 'Alastair Buchanan', topic: 'Your presenter' };
/** The caller from the account banned before (section 9: first name only). */
export const TAVISH: CallerSeed = { name: 'Tavish', topic: 'The ferries' };
