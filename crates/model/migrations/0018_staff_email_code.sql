-- SPDX-License-Identifier: AGPL-3.0-only

-- Staff sign in with a code sent by email as well as with a password (ADR-0127: "an emailed
-- code, where the station has mail"). As the chat keeps a chatter's address (ADR-0125), the
-- address is never stored: `email_hash` is an HMAC-SHA-256 of the lower-case address under the
-- station's `WEAVESIGNAL_CONTROL_EMAIL_KEY`, and the person types the address at sign-in; the code
-- is sent only if it matches. A copy of the database alone cannot be tested against a list of
-- addresses. One address belongs to one account.
ALTER TABLE staff_account
    ADD COLUMN email_hash bytea CHECK (email_hash IS NULL OR length(email_hash) = 32);
CREATE UNIQUE INDEX staff_account_email ON staff_account (station_id, email_hash)
    WHERE email_hash IS NOT NULL;

-- A code asked for: six digits kept as a keyed hash that names the account and the address,
-- ten minutes to live, five tries, used once. Asking for a new code spends the older ones.
CREATE TABLE staff_signin (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id  uuid        NOT NULL REFERENCES staff_account (id) ON DELETE CASCADE,
    code_hash   bytea       NOT NULL CHECK (length(code_hash) = 32),
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    attempts    smallint    NOT NULL DEFAULT 0,
    used        boolean     NOT NULL DEFAULT false
);
CREATE INDEX staff_signin_account ON staff_signin (account_id, created_at);
