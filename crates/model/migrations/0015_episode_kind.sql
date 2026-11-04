-- SPDX-License-Identifier: AGPL-3.0-only

-- An episode is live or pre-recorded (ADR-0188, `scr-studios-kind` b). Every episode drafted
-- before this was live, and a new one is live until someone marks it otherwise. Studios, the
-- other half of that decision, wait for a station with several: no column for them.
ALTER TABLE episode
    ADD COLUMN kind text NOT NULL DEFAULT 'live' CHECK (kind IN ('live', 'prerecorded'));
