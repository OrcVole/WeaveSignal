-- SPDX-License-Identifier: AGPL-3.0-only

-- The phone line's two settings an admin may change from the studio (Phase 17j's Settings ›
-- Phone line): whether the consent words play (`phone.consent_words`) and which shipped
-- recording plays until the station records its own (`phone.default_voice`). Each is NULL until
-- an admin sets it here, and then wins over the installation's setting (`PHONE_CONSENT_WORDS`,
-- `PHONE_DEFAULT_VOICE`), as the setup page's name and contact win over the settings file's
-- (`station_setup`, migration 0028). One row per station.
CREATE TABLE phone_settings (
    station_id    uuid        PRIMARY KEY REFERENCES station (id),
    consent_words boolean,
    default_voice text        CHECK (default_voice IN ('alba', 'edinburgh', 'own')),
    updated_by    text        NOT NULL CHECK (length(updated_by) BETWEEN 1 AND 120),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
