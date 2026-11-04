-- SPDX-License-Identifier: AGPL-3.0-only
-- The first migration: the station itself (ADR-0047).
-- Release one shows one station, but every table that follows carries a station_id
-- referencing this table, so several stations per install need no rewrite later.
CREATE TABLE station (
    id         uuid        PRIMARY KEY,
    name       text        NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now()
);
