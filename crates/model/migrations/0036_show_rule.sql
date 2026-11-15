-- SPDX-License-Identifier: AGPL-3.0-only

-- A show's recurrence rule (release 1.0.4, ADR-0278; design new-show.md section 9). A rule
-- replaces the weekly slot on a show and on a series: a typed JSON value (`Rule` in
-- crates/model/src/rule.rs) with a start date, a start time, a length, a repeat (once, weekly,
-- monthly, yearly, chosen dates), an end, skipped dates and moved dates. NULL on a series
-- follows its show, as the slot did.
ALTER TABLE show ADD COLUMN rule jsonb;
ALTER TABLE series ADD COLUMN rule jsonb;

-- The old weekly slot becomes the slot-shaped rule: every week, one weekday, no start date, no
-- end, nothing skipped or moved. The three slot columns stay for one release, no longer read
-- (the routes still write them while the rule is slot-shaped, so a rollback keeps its slots);
-- the release after drops them.
UPDATE show SET rule = jsonb_build_object(
    'starts_on', NULL,
    'start', slot_start,
    'minutes', slot_minutes,
    'repeat', jsonb_build_object('kind', 'weekly', 'every', 1, 'weekdays', jsonb_build_array(slot_weekday)),
    'end', jsonb_build_object('kind', 'never'),
    'skip', '[]'::jsonb,
    'moves', '[]'::jsonb)
 WHERE slot_weekday IS NOT NULL;
UPDATE series SET rule = jsonb_build_object(
    'starts_on', NULL,
    'start', slot_start,
    'minutes', slot_minutes,
    'repeat', jsonb_build_object('kind', 'weekly', 'every', 1, 'weekdays', jsonb_build_array(slot_weekday)),
    'end', jsonb_build_object('kind', 'never'),
    'skip', '[]'::jsonb,
    'moves', '[]'::jsonb)
 WHERE slot_weekday IS NOT NULL;

-- The rule date an episode fills, as the station's local date of the nominal start. One start a
-- day makes the date enough. Drafting sets it; moving the episode in the week keeps it (that is
-- what makes the block MOVED); an episode made or left by hand has none. The existing episodes
-- of a series with a slot are filled in by the control plane at its first start after this
-- migration (`shows::backfill_occurrences`): the date needs the station's zone, which is
-- settled there and not in the database.
-- Deferrable, so that several episodes can swap dates in one transaction.
ALTER TABLE episode ADD COLUMN occurrence date;
ALTER TABLE episode
    ADD CONSTRAINT episode_occurrence_key UNIQUE (series_id, occurrence) DEFERRABLE INITIALLY IMMEDIATE;

-- Backfills that need more than SQL, done once each (see `schema_backfill`).
CREATE TABLE schema_backfill (
    name    text        PRIMARY KEY,
    done_at timestamptz NOT NULL DEFAULT now()
);

-- Each change of a show's or a series' rule, with who made it and when, as a change of step is
-- logged (migration 0033).
CREATE TABLE show_rule_change (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    show_id     uuid        NOT NULL REFERENCES show (id) ON DELETE CASCADE,
    series_id   uuid        REFERENCES series (id) ON DELETE SET NULL,
    from_rule   jsonb,
    to_rule     jsonb,
    -- What was done with the drafted episodes: `move` or `leave`; `moved` counts those moved.
    drafted     text        NOT NULL CHECK (drafted IN ('move', 'leave')),
    moved       integer     NOT NULL DEFAULT 0,
    by_label    text        NOT NULL CHECK (length(by_label) BETWEEN 1 AND 160),
    by_account  uuid        REFERENCES staff_account (id) ON DELETE SET NULL,
    at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX show_rule_change_show ON show_rule_change (show_id, at);
