// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// The station's one time zone (ADR-0189): what Settings checks before it asks the station to
// take a name, and the words it shows. The station is the authority and refuses a name that is
// not an IANA zone; this only saves a round trip for a typing slip.

/** The zones this window knows, for the drop-down's suggestions; empty where `Intl` cannot list
 *  them (the station still checks whatever is typed). */
export function knownZones(): string[] {
  const intl = Intl as unknown as { supportedValuesOf?: (k: string) => string[] };
  try {
    return intl.supportedValuesOf?.('timeZone') ?? [];
  } catch {
    return [];
  }
}

/** Why `name` cannot be the station's zone, or null. `known` is the list to check against; an
 *  empty list checks nothing but that a name was typed. */
export function zoneProblem(name: string, known: readonly string[]): string | null {
  const n = name.trim();
  if (!n) return 'Choose a time zone, such as Europe/London.';
  if (known.length && !known.includes(n)) {
    return `“${n}” is not a time zone this app knows. Use an IANA name such as Europe/London or America/New_York.`;
  }
  return null;
}

/** The warning before a change that moves every stored grid. Shows and clock hours are kept as
 *  a weekday and an hour on the wall clock, with no zone, so the same hour in another zone is
 *  another moment. */
export const ZONE_CHANGE_WARNING =
  'Every show slot and every clock hour is stored as a weekday and an hour on the wall clock, with no zone. After this change each one airs at that same hour in the new zone, which is a different moment from before.';
