-- SPDX-License-Identifier: AGPL-3.0-only

-- Keyed shows (1.3.2, PLAN.md Phase 19b; docs/reference/design/keyed-shows.md): shows that are
-- advertised in public and heard or watched only with a key. Numbered 0071 on branch
-- `p132-keys-backend` while `m1`'s highest is 0070: renumber if another branch took 0071.
--
-- Nothing here holds a key's text in the clear: `text_hash` is a keyed hash for finding a key by
-- what a listener types, and `text_sealed` is the text sealed (ChaCha20-Poly1305) so staff can
-- send it again. An email address is kept the same way, with a keyed hash for the lost-key
-- request. A copy of this database alone gives neither.

-- A show's access rule. A show with no row here is public. `opens_at`: from then the show is
-- public whatever the rule says (early access). `slate`: what the public hears while the show is
-- keyed: `announcement` (a slate saying when the public programme returns and how to get a key),
-- `fallback` (the fallback loop) or `teaser`.
CREATE TABLE show_access (
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    show_id     uuid        NOT NULL,
    rule        text        NOT NULL CHECK (rule IN ('public', 'key')),
    opens_at    timestamptz,
    slate       text        NOT NULL DEFAULT 'announcement'
                            CHECK (slate IN ('announcement', 'fallback', 'teaser')),
    slate_note  text        NOT NULL DEFAULT '' CHECK (length(slate_note) <= 500),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    updated_by  text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    PRIMARY KEY (station_id, show_id),
    FOREIGN KEY (station_id, show_id) REFERENCES show (station_id, id) ON DELETE CASCADE
);

-- A key family: a label for keys issued together, one per recipient, so that every copy is its
-- own key and a leak can be traced to the key it came from.
CREATE TABLE listener_key_family (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    label       text        NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    note        text        NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  text        NOT NULL CHECK (length(created_by) BETWEEN 1 AND 120),
    UNIQUE (station_id, id),
    UNIQUE (station_id, label)
);

-- A key. Its kind is `personal` (one named person), `group` (many who share it, up to
-- `max_people`) or `machine` (a relay or partner station: no devices, a stable stream address).
-- Public-by-time is not a key but a show's `opens_at`. `media` is what it opens: `sound`,
-- `video` or `both`. A key opens `all_shows` (every keyed show) or the shows in
-- `listener_key_show`. `valid_from` is inclusive and `valid_until` exclusive; null is no limit.
-- The limits: `max_devices` browsers or apps at once, `max_listens` starts of a listen,
-- `max_people` different devices ever (a group key's people); null is no limit.
CREATE TABLE listener_key (
    id               uuid        PRIMARY KEY,
    station_id       uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    family_id        uuid,
    kind             text        NOT NULL CHECK (kind IN ('personal', 'group', 'machine')),
    label            text        NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    note             text        NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    email_hash       bytea       CHECK (email_hash IS NULL OR length(email_hash) = 32),
    email_nonce      bytea,
    email_sealed     bytea,
    media            text        NOT NULL DEFAULT 'both' CHECK (media IN ('sound', 'video', 'both')),
    all_shows        boolean     NOT NULL DEFAULT false,
    valid_from       timestamptz,
    valid_until      timestamptz,
    max_devices      integer     CHECK (max_devices IS NULL OR max_devices BETWEEN 1 AND 1000),
    max_listens      integer     CHECK (max_listens IS NULL OR max_listens BETWEEN 1 AND 1000000),
    max_people       integer     CHECK (max_people IS NULL OR max_people BETWEEN 1 AND 100000),
    state            text        NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'paused', 'revoked')),
    state_reason     text        NOT NULL DEFAULT '' CHECK (length(state_reason) <= 200),
    state_at         timestamptz,
    text_hash        bytea       NOT NULL CHECK (length(text_hash) = 32),
    text_nonce       bytea       NOT NULL,
    text_sealed      bytea       NOT NULL,
    listens_used     integer     NOT NULL DEFAULT 0,
    first_used_at    timestamptz,
    last_used_at     timestamptz,
    -- Set when `key.ended` was sent, so that it is sent once across restarts; cleared when the
    -- key is extended or resumed.
    ended_notified_at timestamptz,
    replaces_id      uuid,
    replaced_by_id   uuid,
    created_at       timestamptz NOT NULL DEFAULT now(),
    created_by       text        NOT NULL CHECK (length(created_by) BETWEEN 1 AND 120),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, id),
    UNIQUE (station_id, text_hash),
    FOREIGN KEY (station_id, family_id) REFERENCES listener_key_family (station_id, id),
    CHECK (valid_from IS NULL OR valid_until IS NULL OR valid_until > valid_from),
    CHECK ((email_hash IS NULL) = (email_sealed IS NULL) AND (email_hash IS NULL) = (email_nonce IS NULL))
);
CREATE INDEX listener_key_by_email ON listener_key (station_id, email_hash) WHERE email_hash IS NOT NULL;
CREATE INDEX listener_key_by_family ON listener_key (station_id, family_id) WHERE family_id IS NOT NULL;
CREATE INDEX listener_key_to_end ON listener_key (valid_until)
    WHERE ended_notified_at IS NULL AND valid_until IS NOT NULL;

-- The shows a key opens when it is not `all_shows`.
CREATE TABLE listener_key_show (
    station_id uuid NOT NULL,
    key_id     uuid NOT NULL,
    show_id    uuid NOT NULL,
    PRIMARY KEY (key_id, show_id),
    FOREIGN KEY (station_id, key_id) REFERENCES listener_key (station_id, id) ON DELETE CASCADE,
    FOREIGN KEY (station_id, show_id) REFERENCES show (station_id, id) ON DELETE CASCADE
);
CREATE INDEX listener_key_show_by_show ON listener_key_show (show_id);

-- Who changed what and when (and when a key was revealed, resent, asked for). No key text.
CREATE TABLE listener_key_log (
    id         bigserial   PRIMARY KEY,
    station_id uuid        NOT NULL,
    key_id     uuid        NOT NULL,
    at         timestamptz NOT NULL DEFAULT now(),
    actor      text        NOT NULL CHECK (length(actor) BETWEEN 1 AND 120),
    action     text        NOT NULL CHECK (length(action) BETWEEN 1 AND 40),
    detail     jsonb       NOT NULL DEFAULT '{}',
    FOREIGN KEY (station_id, key_id) REFERENCES listener_key (station_id, id) ON DELETE CASCADE
);
CREATE INDEX listener_key_log_by_key ON listener_key_log (key_id, at DESC, id DESC);

-- A browser or app that holds a key. `ended_at` is set when the listener (or staff) signs it
-- out; it is then refused at its next renewal.
CREATE TABLE listener_key_device (
    id            uuid        PRIMARY KEY,
    station_id    uuid        NOT NULL,
    key_id        uuid        NOT NULL,
    label         text        NOT NULL DEFAULT '' CHECK (length(label) <= 120),
    first_seen_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at  timestamptz NOT NULL DEFAULT now(),
    ended_at      timestamptz,
    last_show_id  uuid,
    FOREIGN KEY (station_id, key_id) REFERENCES listener_key (station_id, id) ON DELETE CASCADE
);
CREATE INDEX listener_key_device_by_key ON listener_key_device (key_id, last_seen_at DESC);

-- Use records for the leak report: one row per key, device, hashed address and hour. The
-- address is a keyed hash of the address (an IPv6 one folded to its /64) cut to 8 bytes: it
-- tells addresses apart and cannot be turned back into one. Pruned after `keys.use_records_days`.
CREATE TABLE listener_key_use (
    station_id uuid        NOT NULL,
    key_id     uuid        NOT NULL,
    device_id  uuid        NOT NULL,
    addr_hash  bytea       NOT NULL CHECK (length(addr_hash) = 8),
    hour       timestamptz NOT NULL,
    first_at   timestamptz NOT NULL,
    last_at    timestamptz NOT NULL,
    uses       integer     NOT NULL DEFAULT 1,
    PRIMARY KEY (key_id, device_id, addr_hash, hour),
    FOREIGN KEY (station_id, key_id) REFERENCES listener_key (station_id, id) ON DELETE CASCADE
);
CREATE INDEX listener_key_use_by_hour ON listener_key_use (hour);
