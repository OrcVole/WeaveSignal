-- SPDX-License-Identifier: AGPL-3.0-only

-- Hosts in their own studios (ADR-0280, ADR-0284; design section 10): a show key for each
-- person who sends to a show from their own software, where a show comes from, what happened
-- to each connection, and the station's rules for sources.
--
-- A key's secret is never kept: it is derived under the token key from the key id and the salt
-- here (as a webhook's signing secret is), so a copy of this database is not a set of working
-- keys; the SRT listener needs the secret in plain to answer the handshake, so a one-way hash
-- alone could not serve. The monitor link's secret is only ever checked, so its keyed hash is
-- kept. `key_id` is public: it is the SRT stream id and the first part of the Icecast password.
CREATE TABLE show_key (
    id                   uuid        PRIMARY KEY,
    station_id           uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    show_id              uuid        NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    label                text        NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    staff_id             uuid        REFERENCES staff_account (id) ON DELETE SET NULL,
    key_id               text        NOT NULL UNIQUE CHECK (key_id ~ '^[a-z2-7]{8}$'),
    key_salt             bytea       NOT NULL CHECK (length(key_salt) = 16),
    monitor_hash         bytea       NOT NULL CHECK (length(monitor_hash) = 32),
    hint                 text        NOT NULL CHECK (hint IN ('icecast', 'srt')),
    created_by           text        NOT NULL CHECK (length(created_by) BETWEEN 1 AND 120),
    created_ms           bigint      NOT NULL,
    last_used_ms         bigint,
    -- Ended for good: refused from then on.
    revoked_ms           bigint,
    -- Made again with `old: on-disconnect`: this key ends when its connection closes.
    ends_on_disconnect   boolean     NOT NULL DEFAULT false,
    -- Disconnect on the tile turns the key away for the rest of that date's slot.
    turned_away_until_ms bigint,
    turned_away_by       text        CHECK (length(turned_away_by) <= 120)
);

CREATE INDEX show_key_show ON show_key (station_id, show_id);

-- Where a show comes from: the station's room (as before), or a host's own software.
ALTER TABLE show
    ADD COLUMN source text NOT NULL DEFAULT 'room' CHECK (source IN ('room', 'push'));

-- What happened to each source connection, for the key's row in Settings and the station's
-- log: admitted, a test, refused and why, ended. Rows (and the addresses in them) are kept 30
-- days; the control plane deletes older ones. `reason` is a word from the refusals' list.
CREATE TABLE source_log (
    id         bigserial   PRIMARY KEY,
    station_id uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    at_ms      bigint      NOT NULL,
    key_id     text        NOT NULL CHECK (length(key_id) <= 80),
    show_id    uuid        REFERENCES show (id) ON DELETE SET NULL,
    event      text        NOT NULL CHECK (event IN ('admitted', 'test', 'refused', 'ended', 'disconnected', 'let_back')),
    reason     text        CHECK (length(reason) <= 60),
    detail     text        NOT NULL DEFAULT '' CHECK (length(detail) <= 400),
    way        text        CHECK (way IN ('icecast', 'srt')),
    address    text        CHECK (length(address) <= 64)
);

CREATE INDEX source_log_key ON source_log (station_id, key_id, at_ms DESC);

-- The station's rules for sources (design section 10). No row means the defaults. The two ports
-- are the ingest's public listeners, off until the first key exists.
CREATE TABLE source_settings (
    station_id      uuid    PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    door_before_min integer NOT NULL DEFAULT 30   CHECK (door_before_min BETWEEN 0 AND 240),
    grace_s         integer NOT NULL DEFAULT 120  CHECK (grace_s IN (0, 60, 120, 300)),
    late_count_s    integer NOT NULL DEFAULT 20   CHECK (late_count_s BETWEEN 0 AND 120),
    test_outside    boolean NOT NULL DEFAULT true,
    test_max_min    integer NOT NULL DEFAULT 15   CHECK (test_max_min BETWEEN 1 AND 60),
    steady_s        integer NOT NULL DEFAULT 5    CHECK (steady_s BETWEEN 1 AND 60),
    silence_s       integer NOT NULL DEFAULT 4    CHECK (silence_s BETWEEN 1 AND 30),
    place_kept_s    integer NOT NULL DEFAULT 300  CHECK (place_kept_s BETWEEN 0 AND 3600),
    fade_ms         integer NOT NULL DEFAULT 2000 CHECK (fade_ms BETWEEN 0 AND 10000),
    port_icecast    integer NOT NULL DEFAULT 8005 CHECK (port_icecast BETWEEN 1 AND 65535),
    port_srt        integer NOT NULL DEFAULT 9710 CHECK (port_srt BETWEEN 1 AND 65535),
    updated_by      text    NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_ms      bigint  NOT NULL
);
