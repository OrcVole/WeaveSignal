-- SPDX-License-Identifier: AGPL-3.0-only

-- API tokens (ADR-0195): a named, scoped credential for an integration, so a station never hands
-- out its station key. The secret is shown once, when the token is made; only a keyed hash of it
-- is kept (HMAC-SHA-256 under the station's token key, `WEAVESIGNAL_CONTROL_TOKEN_KEY`), so a copy
-- of the database alone cannot be used as a token. `prefix` is the first characters of the
-- secret, kept so an admin can tell tokens apart in a list. A revoked token stays in the list
-- (with when and by whom it was made) and never works again; `last_used_at` is written at most
-- once a minute.
CREATE TABLE api_token (
    id           uuid        PRIMARY KEY,
    station_id   uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    name         text        NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    scopes       text[]      NOT NULL CHECK (cardinality(scopes) BETWEEN 1 AND 64),
    prefix       text        NOT NULL CHECK (length(prefix) BETWEEN 4 AND 16),
    secret_hash  bytea       NOT NULL UNIQUE CHECK (length(secret_hash) = 32),
    created_by   text        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz,
    last_used_at timestamptz,
    revoked_at   timestamptz
);
CREATE INDEX api_token_station ON api_token (station_id, created_at);
