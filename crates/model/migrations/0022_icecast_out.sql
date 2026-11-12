-- SPDX-License-Identifier: AGPL-3.0-only

-- The outgoing Icecast source (ADR-0226): whether the programme is sent to another Icecast
-- server (a DJ's AzuraCast) by itself, always, or never. The server's address and password are
-- the deployment's settings (the listener process reads them), not rows here. No row means
-- `auto`: send while the station is live with the desk on air.
CREATE TABLE icecast_out_setting (
    station_id uuid        PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    mode       text        NOT NULL CHECK (mode IN ('auto', 'on', 'off')),
    updated_by text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_at timestamptz NOT NULL DEFAULT now()
);
