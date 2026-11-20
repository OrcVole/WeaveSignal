-- SPDX-License-Identifier: AGPL-3.0-only

-- The telephone line (Phase 17f). Numbered 0052 on branch `phone-line`, renumbered 0046 when the
-- 1.2 branches merged, after the other Phase 17 branches' migrations.
--
-- **No phone number is ever stored.** A phone caller is kept as a ban key (`pb_` and 32 hex
-- digits: an HMAC, under a key the station holds outside the database, of LiveKit's hashed
-- caller identity) and the last digits LiveKit left in the room's name (at most four). A
-- withheld caller has no ban key: every withheld caller shares one identity.

-- A consent says which line it came by, and for a phone call its ban key (so a takedown by
-- number finds the call) and its last digits. `not_asked`: the station has the phone words
-- turned off, so nothing was played or agreed; the record says so rather than claim consent.
ALTER TABLE caller_consent ADD COLUMN line text NOT NULL DEFAULT 'browser'
    CHECK (line IN ('browser', 'phone'));
ALTER TABLE caller_consent ADD COLUMN ban_key text CHECK (ban_key ~ '^pb_[0-9a-f]{32}$');
ALTER TABLE caller_consent ADD COLUMN number_tail text CHECK (length(number_tail) <= 4);
ALTER TABLE caller_consent ADD CONSTRAINT caller_consent_phone_fields
    CHECK (line = 'phone' OR (ban_key IS NULL AND number_tail IS NULL));
ALTER TABLE caller_consent DROP CONSTRAINT caller_consent_given_by_check;
ALTER TABLE caller_consent ADD CONSTRAINT caller_consent_given_by_check
    CHECK (given_by IN ('caller', 'station', 'not_asked'));
CREATE INDEX caller_consent_ban_key ON caller_consent (station_id, ban_key)
    WHERE ban_key IS NOT NULL;

-- A number's ban: by ban key, with the last digits as the screener saw them, the day, the
-- reason and who made it. It lapses with the call-in retention preset unless renewed.
CREATE TABLE phone_ban (
    station_id  uuid        NOT NULL REFERENCES station (id),
    ban_key     text        NOT NULL CHECK (ban_key ~ '^pb_[0-9a-f]{32}$'),
    number_tail text        NOT NULL CHECK (length(number_tail) <= 4),
    reason      text        NOT NULL CHECK (length(reason) <= 280),
    made_by     text        NOT NULL CHECK (length(made_by) BETWEEN 1 AND 120),
    made_at     timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz,
    PRIMARY KEY (station_id, ban_key)
);

-- The station's own recording of the phone words (phone-words a): the words it was made from
-- (the browser's consent words, filled, and the key sentence), their version, and the file in
-- the phone-words directory. One per station; a new recording replaces it. With none, the
-- recording that ships with WeaveSignal plays.
CREATE TABLE phone_words (
    station_id    uuid        PRIMARY KEY REFERENCES station (id),
    words         text        NOT NULL CHECK (length(words) BETWEEN 1 AND 4000),
    words_version text        NOT NULL CHECK (length(words_version) = 16),
    file          text        NOT NULL CHECK (file ~ '^[0-9a-f-]{36}\.flac$'),
    duration_ms   integer     NOT NULL CHECK (duration_ms BETWEEN 500 AND 120000),
    -- The media worker's play gain: the desk plays the file at it, as it plays a cart.
    gain_db       real        NOT NULL DEFAULT 0 CHECK (gain_db BETWEEN -60 AND 30),
    recorded_by   text        NOT NULL CHECK (length(recorded_by) BETWEEN 1 AND 120),
    recorded_at   timestamptz NOT NULL DEFAULT now()
);
