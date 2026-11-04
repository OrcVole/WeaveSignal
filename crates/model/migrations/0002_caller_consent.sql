-- SPDX-License-Identifier: AGPL-3.0-only
-- Callers' consent to record (M3): who agreed, when, and to which words, kept before the
-- control plane signs a caller's room token. One row per consent given; the words are stored
-- whole beside their version (the first 16 hex digits of their SHA-256), so a record still
-- says what was agreed after the station changes its wording. How long rows are kept is not
-- decided yet (no ADR); nothing here deletes them.
CREATE TABLE caller_consent (
    id           bigserial   PRIMARY KEY,
    station_id   uuid        NOT NULL REFERENCES station (id),
    identity     text        NOT NULL CHECK (length(identity) BETWEEN 1 AND 64),
    name         text        NOT NULL CHECK (length(name) <= 120),
    topic        text        NOT NULL CHECK (length(topic) <= 280),
    given_by     text        NOT NULL CHECK (given_by IN ('caller', 'station')),
    words        text        NOT NULL CHECK (length(words) BETWEEN 1 AND 4000),
    words_version text       NOT NULL CHECK (length(words_version) = 16),
    accepted_at  timestamptz NOT NULL
);
CREATE INDEX caller_consent_identity ON caller_consent (station_id, identity);
