-- SPDX-License-Identifier: AGPL-3.0-only

-- A show runs at one of four beginner steps (ADR-0271): `solo` (one DJ), `guests`, `callers` or
-- `full`. Whatever the step leaves out is switched off while the show is on air. The shows that
-- exist when this migration runs stay `full`, so an upgrade hides nothing; a show made
-- afterwards without the field starts at `solo` (the default is set after the existing rows
-- have been filled).
ALTER TABLE show
    ADD COLUMN step text NOT NULL DEFAULT 'full'
        CHECK (step IN ('solo', 'guests', 'callers', 'full'));
ALTER TABLE show ALTER COLUMN step SET DEFAULT 'solo';

-- Each change of a show's step, with who made it and when (the log the design asks for). A
-- change made by the show editor's save, by the step route, or by an integration's save all
-- land here. `by_label` is how the control plane's logs name the actor (a username, or
-- `station key`); the account is kept while it exists.
CREATE TABLE show_step_change (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    show_id     uuid        NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    from_step   text        NOT NULL CHECK (from_step IN ('solo', 'guests', 'callers', 'full')),
    to_step     text        NOT NULL CHECK (to_step IN ('solo', 'guests', 'callers', 'full')),
    by_label    text        NOT NULL CHECK (length(by_label) BETWEEN 1 AND 160),
    by_account  uuid        REFERENCES staff_account (id) ON DELETE SET NULL,
    at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX show_step_change_show ON show_step_change (show_id, at);

-- The desk's chat glance counts held messages on every look (the studio's view carries the
-- count): a partial index keeps that cheap on a busy chat.
CREATE INDEX chat_message_held ON chat_message (station_id) WHERE state = 'held';
