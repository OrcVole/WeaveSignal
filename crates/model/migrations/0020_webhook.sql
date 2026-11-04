-- SPDX-License-Identifier: AGPL-3.0-only

-- Webhooks (ADR-0195): a URL that is told about a small set of events, each delivery signed
-- with `X-WeaveSignal-Signature: sha256=<HMAC of the body>`. The signing secret is never stored:
-- it is derived (HMAC-SHA-256 under the station's token key) from the webhook's id and its
-- `salt`, so a copy of the database alone cannot sign anything; rotating the secret changes the
-- salt. The last delivery is recorded for the admin to read: `last_status` is the HTTP status
-- the receiver answered with (0 when it did not answer), `failures` the deliveries that have
-- failed since the last good one.
CREATE TABLE webhook (
    id              uuid        PRIMARY KEY,
    station_id      uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    name            text        NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    url             text        NOT NULL CHECK (length(url) BETWEEN 8 AND 2000),
    events          text[]      NOT NULL CHECK (cardinality(events) BETWEEN 1 AND 16),
    salt            bytea       NOT NULL CHECK (length(salt) = 16),
    enabled         boolean     NOT NULL DEFAULT true,
    created_by      text        NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    last_attempt_at timestamptz,
    last_success_at timestamptz,
    last_status     integer,
    last_error      text,
    failures        integer     NOT NULL DEFAULT 0
);
CREATE INDEX webhook_station ON webhook (station_id, created_at);
