-- SPDX-License-Identifier: AGPL-3.0-only

-- Staff accounts (ADR-0127, `top-roles-scoped` a): each member of staff signs in as themselves,
-- with one station-wide role. A password works on every deployment, offline included; the
-- password is kept only as an Argon2id hash in PHC form. `disabled_at` takes an account's
-- powers away at once without losing whose name the logs carry.
CREATE TABLE staff_account (
    id            uuid        PRIMARY KEY,
    station_id    uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    username      text        NOT NULL CHECK (username ~ '^[a-z0-9][a-z0-9._-]{0,39}$'),
    name          text        NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    role          text        NOT NULL CHECK (role IN ('admin', 'producer', 'presenter', 'screener')),
    password_hash text        CHECK (password_hash IS NULL OR password_hash LIKE '$argon2id$%'),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    disabled_at   timestamptz,
    UNIQUE (station_id, username)
);

-- A signed-in staff member: the token is kept only as its SHA-256, as the chat's sessions are.
-- `expires_at` moves forward while the session is in use, never past `created_at` plus the
-- longest a session lives (the control plane holds the live set in memory and writes here).
CREATE TABLE staff_session (
    token_hash  bytea       PRIMARY KEY CHECK (length(token_hash) = 32),
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id  uuid        NOT NULL REFERENCES staff_account (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL
);
CREATE INDEX staff_session_account ON staff_session (account_id);

-- The chat finds a staff member's chat account by their staff account, not by a typed name
-- (`chat-staff-identity` b): the moderation log then names who acted.
ALTER TABLE chat_account
    ADD COLUMN staff_account_id uuid REFERENCES staff_account (id) ON DELETE SET NULL;
CREATE UNIQUE INDEX chat_account_staff ON chat_account (station_id, staff_account_id)
    WHERE staff_account_id IS NOT NULL;
