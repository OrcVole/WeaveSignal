-- SPDX-License-Identifier: AGPL-3.0-only

-- Each show carries its presenter's on-air name and takedown contact (ADR-0214), the links to
-- the presenter's channels, whether it plays commercial music (ADR-0219, a reminder only), and
-- the staff account that presents it (ADR-0218: the wizard's answers are that account's
-- defaults). `presenter_account` follows migration 0021's `made_by_account`: it points at the
-- account while it exists and is cleared when the account goes.
ALTER TABLE show
    ADD COLUMN presenter         text    NOT NULL DEFAULT '' CHECK (length(presenter) <= 120),
    ADD COLUMN contact           text    NOT NULL DEFAULT '' CHECK (length(contact) <= 200),
    ADD COLUMN links             text[]  NOT NULL DEFAULT '{}',
    ADD COLUMN commercial_music  boolean NOT NULL DEFAULT false,
    ADD COLUMN presenter_account uuid    REFERENCES staff_account (id) ON DELETE SET NULL;

CREATE INDEX show_presenter_account ON show (station_id, presenter_account)
    WHERE presenter_account IS NOT NULL;

-- A call's consent keeps the show on air when it was given, and the presenter and the contact
-- phrase exactly as the words said them (ADR-0214): a later change to the show never rewrites
-- what a caller agreed to. `words` already keeps the whole sentence; these keep its parts.
ALTER TABLE caller_consent
    ADD COLUMN show_id   uuid REFERENCES show (id) ON DELETE SET NULL,
    ADD COLUMN presenter text NOT NULL DEFAULT '' CHECK (length(presenter) <= 120),
    ADD COLUMN contact   text NOT NULL DEFAULT '' CHECK (length(contact) <= 600);
