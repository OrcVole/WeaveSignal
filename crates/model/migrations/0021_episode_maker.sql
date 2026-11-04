-- SPDX-License-Identifier: AGPL-3.0-only

-- Who made an episode (ADR-0198): a correction to an aired episode is allowed to admins and to
-- the staff member who made it. `made_by` names the maker as the logs do (a username, or
-- "station key"), kept as text so it survives the account; `made_by_account` points at the staff
-- account while it exists. Episodes made before this have neither, and only admins correct them.
ALTER TABLE episode
    ADD COLUMN made_by         text CHECK (made_by IS NULL OR length(made_by) BETWEEN 1 AND 120),
    ADD COLUMN made_by_account uuid REFERENCES staff_account (id) ON DELETE SET NULL;
