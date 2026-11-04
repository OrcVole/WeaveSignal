-- SPDX-License-Identifier: AGPL-3.0-only

-- A hold extended past its date (ADR-0156). Without one, a hold's "keep until" date is derived
-- from the jurisdiction's complaint rule and the start of the broadcast it covers; extending
-- stores a later date of its own, with who extended it, when and why, so the hold stays its own
-- log. Only the latest extension is kept here; each one is also written to the control plane's
-- log. Extending clears the "past its date" flag until the new date.
ALTER TABLE retention_hold
    ADD COLUMN extended_until  timestamptz,
    ADD COLUMN extended_by     text CHECK (length(extended_by) BETWEEN 1 AND 120),
    ADD COLUMN extended_at     timestamptz,
    ADD COLUMN extend_reason   text CHECK (length(extend_reason) BETWEEN 1 AND 1000),
    ADD CONSTRAINT retention_hold_extension_whole CHECK (
        (extended_until IS NULL) = (extended_by IS NULL)
        AND (extended_until IS NULL) = (extended_at IS NULL)
        AND (extended_until IS NULL) = (extend_reason IS NULL)
    );
