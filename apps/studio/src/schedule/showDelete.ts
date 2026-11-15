// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Delete show (ADR-0279), the words of its confirmation. The station refuses a show that has
// aired or is on air, and says so in its own sentence; the studio only says what goes with a
// show that never aired, with the counts it can read.

/** What the studio can count of a show: null while it has not been read, or could not be. */
export interface GoesWith {
  series: number | null;
  episodes: number | null;
}

/** The counts of a show's series and of the episodes in them that are still drafts (one that has
 *  aired is the station's to refuse, and is not counted as going). */
export function countsOf(series: readonly { episodes: readonly { episode: { aired_at: number | null } }[] }[]): GoesWith {
  return { series: series.length, episodes: series.reduce((n, s) => n + s.episodes.filter((e) => e.episode.aired_at === null).length, 0) };
}

const SIDE = 'cart banks and cards';

/** The line under the heading, before "This cannot be undone": real counts where the studio has
 *  them (`Its 2 series and 7 drafted episodes go with it, and its cart banks and cards.`),
 *  the plain list where it has not. */
export function goesWith(c: GoesWith): string {
  if (c.series === null || c.episodes === null) return `Its series, drafted episodes, ${SIDE} go with it.`;
  if (c.series === 0) return `Its ${SIDE} go with it.`;
  const series = `${c.series} series`;
  if (c.episodes === 0) return `Its ${series} ${c.series === 1 ? 'goes' : 'go'} with it, and its ${SIDE}.`;
  const episodes = `${c.episodes} drafted ${c.episodes === 1 ? 'episode' : 'episodes'}`;
  return `Its ${series} and ${episodes} go with it, and its ${SIDE}.`;
}

/** The toast after a delete (the tab's own line, with no Undo: it cannot be undone). */
export const deletedWords = (name: string): string => `${name} is deleted.`;
