-- SPDX-License-Identifier: AGPL-3.0-only

-- Passes the studio has ended (ADR-0119, reopening ADR-0108): a pass issued at or before
-- `ended_at` for this identity buys no more room tokens, across a restart of the control
-- plane. A row matters only while such a pass could still live (24 h at most), so rows older
-- than a day are deleted as new ones are written and at start; nothing else is kept.
CREATE TABLE pass_ended (
    station_id uuid        NOT NULL REFERENCES station (id),
    identity   text        NOT NULL CHECK (length(identity) BETWEEN 1 AND 64),
    ended_at   timestamptz NOT NULL,
    PRIMARY KEY (station_id, identity)
);
