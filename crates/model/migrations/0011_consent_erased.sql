-- SPDX-License-Identifier: AGPL-3.0-only

-- A caller's consent erased at their request (ADR-0122): the name and topic are redacted, and
-- the record keeps its date, the words agreed and their version, as the evidence that the
-- recording was lawful. Who erased it and when are kept beside it.
ALTER TABLE caller_consent ADD COLUMN erased_at timestamptz;
ALTER TABLE caller_consent ADD COLUMN erased_by text CHECK (length(erased_by) BETWEEN 1 AND 120);
