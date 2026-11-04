-- SPDX-License-Identifier: AGPL-3.0-only

-- The station's one time zone (review 0025, S1). A playout_settings row is created with the
-- column's UTC default whenever anything first reads the settings, so 'UTC' alone cannot say
-- whether anyone chose it. `zone_set` does: the studio's settings set it; until then the
-- control plane takes the configuration's `publishing.zone` at every start.
ALTER TABLE playout_settings ADD COLUMN zone_set boolean NOT NULL DEFAULT false;
