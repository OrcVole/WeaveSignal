-- SPDX-License-Identifier: AGPL-3.0-only

-- RTMP out (ADR-0229, ADR-0230; `rtmp-out`): where the desk sends the programme's picture and
-- sound, one row per destination. The stream key is a secret: it is kept sealed
-- (ChaCha20-Poly1305) under a key the control plane derives from its token key, with the nonce
-- beside it, so a copy of the database alone does not hold the keys. `profile` is `compatible`
-- (H.264 and AAC) or `open` (AV1 and Opus over Enhanced RTMP); `platform` names the form the
-- studio app filled the URL from (`rumble`, `odysee`, `peertube` or `custom`). Off until an
-- admin turns it on.
CREATE TABLE rtmp_destination (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    name        text        NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    platform    text        NOT NULL CHECK (platform IN ('rumble', 'odysee', 'peertube', 'custom')),
    url         text        NOT NULL CHECK (length(url) BETWEEN 9 AND 500),
    key_nonce   bytea       NOT NULL CHECK (length(key_nonce) = 12),
    key_sealed  bytea       NOT NULL CHECK (length(key_sealed) BETWEEN 17 AND 1040),
    profile     text        NOT NULL CHECK (profile IN ('compatible', 'open')),
    enabled     boolean     NOT NULL DEFAULT false,
    created_by  text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (station_id, name)
);
