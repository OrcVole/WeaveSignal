-- SPDX-License-Identifier: AGPL-3.0-only
-- SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

-- A chat message id is never reused (0006). The chat hands ids out in memory from one more
-- than the highest it can find at start-up; once the newest messages are swept or erased,
-- the table alone no longer knows that highest id, and a restart would hand the same ids out
-- again (a decision on a new message could close an old, unrelated report: review 0025, S2).
-- Every delete of messages raises this floor first.
CREATE TABLE chat_message_floor (
    station_id uuid   PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    floor      bigint NOT NULL CHECK (floor >= 0)
);
