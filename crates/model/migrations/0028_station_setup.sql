-- SPDX-License-Identifier: AGPL-3.0-only

-- The station's setup page (ADR-0237): what an admin answered on the running station.
-- The settings file gives the name and the takedown contact their first values
-- (`STATION_NAME`, `CONSENT_CONTACT`); a value here wins over the file, and NULL means the
-- file's is in force. `completed_at` is when an admin pressed Finish: setup is offered until
-- then (and only while the name is still the demo's or there is no contact). No row means
-- nobody has opened the page.
CREATE TABLE station_setup (
    station_id      uuid        PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    name            text        CHECK (name IS NULL OR length(name) BETWEEN 1 AND 200),
    consent_contact text        CHECK (consent_contact IS NULL OR length(consent_contact) BETWEEN 1 AND 200),
    completed_at    timestamptz,
    updated_by      text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_at      timestamptz NOT NULL DEFAULT now()
);
