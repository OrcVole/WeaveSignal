-- SPDX-License-Identifier: AGPL-3.0-only

-- The cart wall's loudness target a station keeps when its settings leave `carts.loudness_lufs`
-- out (1.3.3). Until 1.3.3 that default was -23 LUFS; from 1.3.3 a new station's is -16, the
-- internet-radio norm. Every station already in the database when this runs was made by an
-- earlier release, so it is given -23 here and its carts stay as loud as they were. A station
-- made later has NULL, and the new default. The database is migrated before a station's row is
-- written (`ensure_station`), so a new installation's own station is never in this UPDATE. A
-- target set in the settings, or in `CARTS_LOUDNESS_LUFS`, wins over both.
ALTER TABLE station
    ADD COLUMN carts_loudness_default real
        CHECK (carts_loudness_default IS NULL OR carts_loudness_default BETWEEN -40 AND -5);
UPDATE station SET carts_loudness_default = -23;
