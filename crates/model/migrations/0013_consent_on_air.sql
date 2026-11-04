-- SPDX-License-Identifier: AGPL-3.0-only

-- When a caller was on air (ADR-0153): a programme recording keeps the consent only of the
-- calls that went on air during it. One row per stretch on air, against the consent given for
-- the call, so a caller put on air, taken off and put on again has two. `to_at` is null while
-- they are on air; the control plane closes the stretch when they come off, when the call
-- ends, and for any left open by a restart it did not see (it writes the desk's state again at
-- its first poll). The rows go with the consent they belong to.
CREATE TABLE consent_on_air (
    id         bigserial   PRIMARY KEY,
    consent_id bigint      NOT NULL REFERENCES caller_consent (id) ON DELETE CASCADE,
    from_at    timestamptz NOT NULL DEFAULT now(),
    to_at      timestamptz CHECK (to_at >= from_at)
);
CREATE INDEX consent_on_air_consent ON consent_on_air (consent_id);
-- At most one open stretch per consent, so writing the same state twice changes nothing.
CREATE UNIQUE INDEX consent_on_air_open ON consent_on_air (consent_id) WHERE to_at IS NULL;
