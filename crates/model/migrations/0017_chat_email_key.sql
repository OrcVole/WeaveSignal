-- SPDX-License-Identifier: AGPL-3.0-only
-- The chat keeps an email address as a keyed hash (ADR-0125, ADR-0144). Until now it was a plain
-- SHA-256 of the lower-case address, which anyone holding a copy of the database can test
-- against a list of addresses. `email_hash_version` says which a row holds:
--   1  the plain SHA-256 (an account made before this migration);
--   2  HMAC-SHA-256 under the station's `WEAVESIGNAL_CHAT_EMAIL_KEY`.
-- The address itself is never stored, so an old row cannot be converted here: the chat finds it
-- by the plain hash when its owner signs in with a code sent to that address, and re-keys it
-- then, in one statement. Rows with no hash (guests, staff, erased accounts) have nothing to
-- convert and are marked 2.
--
-- The default stays 1, and the chat names the version on every insert: a row written without
-- one can only come from a chat built before this migration (still running while another
-- service migrated), and that chat wrote the plain hash. Marked 1, it is re-keyed like any
-- other; marked 2, it would keep its plain hash for good and never be found again.

ALTER TABLE chat_account
    ADD COLUMN email_hash_version smallint NOT NULL DEFAULT 1
        CHECK (email_hash_version IN (1, 2));
UPDATE chat_account SET email_hash_version = 2 WHERE email_hash IS NULL;

-- The accounts still to be re-keyed, for the start-up log.
CREATE INDEX chat_account_legacy_hash ON chat_account (station_id)
    WHERE email_hash IS NOT NULL AND email_hash_version = 1;

-- A sign-in code lives ten minutes, and its row holds an address hash and a code hash in the
-- old, unkeyed form: drop them (a chatter asks for a new code) instead of carrying two forms.
DELETE FROM chat_signin;
