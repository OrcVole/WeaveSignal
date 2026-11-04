-- SPDX-License-Identifier: AGPL-3.0-only

-- The corrections log (ADR-0177, `cards-frozen` b). An aired episode's frozen record stays as
-- broadcast; staff can publish a correction to its public text (title, description, hosts,
-- guests, tags), which changes what its cards and the feed say under the same episode id.
-- Each correction keeps who made it, why, and the changed fields as they were and as they
-- became, so the original stays in the log. An erasure (ADR-0122) redacts a person's name
-- here too, and says so in `redacted_at`.
--
-- `made_by` is how the log names the actor (a username, or "station key"), kept as text so it
-- survives the account; `made_by_account` points at the staff account while it exists.
CREATE TABLE episode_correction (
    id              uuid        PRIMARY KEY,
    station_id      uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    episode_id      uuid        NOT NULL REFERENCES episode (id) ON DELETE CASCADE,
    made_at         timestamptz NOT NULL DEFAULT now(),
    made_by         text        NOT NULL CHECK (length(made_by) BETWEEN 1 AND 120),
    made_by_account uuid        REFERENCES staff_account (id) ON DELETE SET NULL,
    -- At most 500 characters as written (`CorrectionInput::check`); no upper bound here, so an
    -- erasure that puts "[erased]" in place of a short name can never be refused.
    reason          text        NOT NULL CHECK (length(reason) >= 1),
    old_values      jsonb       NOT NULL,
    new_values      jsonb       NOT NULL,
    redacted_at     timestamptz
);
CREATE INDEX episode_correction_episode ON episode_correction (episode_id, made_at);
