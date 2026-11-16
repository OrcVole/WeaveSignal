// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
// Where a show's rule is read and saved, for the changes one date makes (Skip this date in the
// week; Skip, Put back, Move… and Back to the rule in a show's dates): the series' own rule
// when it has one, else the show's, saved on its own route with the drafted episodes left where
// they are. One place, so every such change reads the rule first.

import * as api from '../lib/schedule.ts';
import * as rulesApi from '../lib/api.ts';
import type { RuleStore } from './dates.ts';

export const stationRules: RuleStore = {
  async read(slug, series) {
    const view = await api.show(slug);
    const own = series !== null ? (view.series.find((x) => x.number === series)?.rule ?? null) : null;
    const rule = own ?? view.show.rule ?? null;
    return rule ? { rule, series: own ? series : null } : null;
  },
  async write(slug, series, rule) {
    if (series !== null) await rulesApi.putSeriesRule(slug, series, rule, 'leave');
    else await rulesApi.putShowRule(slug, rule, 'leave');
  },
};
