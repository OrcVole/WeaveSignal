-- SPDX-License-Identifier: AGPL-3.0-only
-- The live chat (M6, ADR-0031 to ADR-0040): light accounts, the room's messages with two
-- timestamps, reports and notices, the moderation log, statements of reasons, restrictions and
-- evidence holds. Every row carries its station (ADR-0047). How long each kind of row is kept
-- is the station's retention setting (ADR-0039, ADR-0113); the chat service's sweeper deletes,
-- nothing here does. 0004 and 0005 belong to other branches.

-- A light account (ADR-0034): email sign-in, or a guest where the station allows guests.
-- The address is kept only as a SHA-256 of its lower-case form: enough to sign the same person
-- in again, nothing to mail or leak. `standing` is what the station has granted: trusted
-- chatters may post links; moderators act on the room.
CREATE TABLE chat_account (
    id              uuid        PRIMARY KEY,
    station_id      uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    kind            text        NOT NULL CHECK (kind IN ('email', 'guest', 'staff')),
    email_hash      bytea       CHECK (email_hash IS NULL OR length(email_hash) = 32),
    name            text        NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    standing        text        NOT NULL DEFAULT 'member'
                                CHECK (standing IN ('member', 'trusted', 'moderator', 'staff')),
    age_declared_at timestamptz,
    terms_version   text        CHECK (terms_version IS NULL OR length(terms_version) <= 32),
    created_at      timestamptz NOT NULL DEFAULT now(),
    erased_at       timestamptz
);
CREATE UNIQUE INDEX chat_account_email ON chat_account (station_id, email_hash)
    WHERE email_hash IS NOT NULL;
CREATE UNIQUE INDEX chat_account_name ON chat_account (station_id, lower(name))
    WHERE erased_at IS NULL AND kind <> 'staff';

-- A signed-in browser: the token is kept only as its SHA-256. `address` is the client's IP,
-- kept only as long as the station's retention for addresses (none by default, ADR-0115).
CREATE TABLE chat_session (
    token_hash  bytea       PRIMARY KEY CHECK (length(token_hash) = 32),
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id  uuid        NOT NULL REFERENCES chat_account (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    address     inet
);
CREATE INDEX chat_session_account ON chat_session (account_id);

-- A sign-in code sent by email: six digits, kept as a hash, a few tries, minutes to live.
CREATE TABLE chat_signin (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    email_hash  bytea       NOT NULL CHECK (length(email_hash) = 32),
    code_hash   bytea       NOT NULL CHECK (length(code_hash) = 32),
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    attempts    smallint    NOT NULL DEFAULT 0,
    used        boolean     NOT NULL DEFAULT false
);
CREATE INDEX chat_signin_email ON chat_signin (station_id, email_hash);

-- A message. `id` is the station's own sequence, never reused. Two timestamps from the first
-- schema (ADR-0038): the server's clock, and the sender's playhead (the programme time the
-- sender was seeing, from the player; null where the player had none). `studio` messages are
-- private to the screener and the presenter (ADR-0040). `held` messages are seen only by
-- moderators until released; `flagged` names the automated step that held one (rustrict).
CREATE TABLE chat_message (
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    id          bigint      NOT NULL,
    account_id  uuid        REFERENCES chat_account (id) ON DELETE SET NULL,
    kind        text        NOT NULL CHECK (kind IN ('chat', 'request', 'studio', 'announcement')),
    body        text        NOT NULL CHECK (length(body) BETWEEN 1 AND 2000),
    reply_to    bigint,
    server_at   timestamptz NOT NULL,
    playhead_at timestamptz,
    state       text        NOT NULL CHECK (state IN ('visible', 'held', 'hidden')),
    flagged     text        CHECK (flagged IS NULL OR length(flagged) <= 40),
    pinned      boolean     NOT NULL DEFAULT false,
    featured_at timestamptz,
    PRIMARY KEY (station_id, id)
);
CREATE INDEX chat_message_account ON chat_message (station_id, account_id);
CREATE INDEX chat_message_time ON chat_message (station_id, server_at);

-- A report from a chatter, a notice from someone without an account (DSA Article 16), or a
-- review a chatter asked for after a statement of reasons. `evidence` is the reported message
-- as it was when reported, so deciding never depends on it still existing. `due_at` is set
-- where a rule fixes a clock (intimate images: 48 hours).
CREATE TABLE chat_report (
    id            uuid        PRIMARY KEY,
    station_id    uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    kind          text        NOT NULL CHECK (kind IN ('report', 'notice', 'review')),
    message_id    bigint,
    account_id    uuid        REFERENCES chat_account (id) ON DELETE SET NULL,
    reporter_id   uuid        REFERENCES chat_account (id) ON DELETE SET NULL,
    reason        text        NOT NULL CHECK (reason IN ('abuse', 'spam', 'off_topic', 'illegal',
                                  'intimate_image', 'threat_to_life', 'child_safety', 'other')),
    detail        text        NOT NULL DEFAULT '' CHECK (length(detail) <= 2000),
    location      text        NOT NULL DEFAULT '' CHECK (length(location) <= 500),
    contact_name  text        NOT NULL DEFAULT '' CHECK (length(contact_name) <= 120),
    contact_email text        NOT NULL DEFAULT '' CHECK (length(contact_email) <= 254),
    evidence      jsonb       NOT NULL DEFAULT '{}',
    statement_id  uuid,
    created_at    timestamptz NOT NULL DEFAULT now(),
    due_at        timestamptz,
    status        text        NOT NULL DEFAULT 'open'
                              CHECK (status IN ('open', 'actioned', 'dismissed', 'acknowledged', 'escalated')),
    decided_at    timestamptz,
    decided_by    text        CHECK (decided_by IS NULL OR length(decided_by) <= 120)
);
CREATE INDEX chat_report_open ON chat_report (station_id, status, created_at);

-- The moderation log: every action with who, why and when, and whether an automated step
-- was involved (DSA Article 17(3)(c)).
CREATE TABLE chat_action (
    id          bigserial   PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    at          timestamptz NOT NULL DEFAULT now(),
    actor       text        NOT NULL CHECK (length(actor) BETWEEN 1 AND 120),
    action      text        NOT NULL CHECK (length(action) BETWEEN 1 AND 40),
    account_id  uuid        REFERENCES chat_account (id) ON DELETE SET NULL,
    message_id  bigint,
    report_id   uuid,
    reason      text        NOT NULL DEFAULT '' CHECK (length(reason) <= 40),
    ground      text        NOT NULL DEFAULT '' CHECK (length(ground) <= 300),
    note        text        NOT NULL DEFAULT '' CHECK (length(note) <= 2000),
    automated   boolean     NOT NULL DEFAULT false,
    until       timestamptz
);
CREATE INDEX chat_action_time ON chat_action (station_id, at);

-- A statement of reasons (DSA Article 17), sent to the chatter at once; they may ask for a
-- review within the window it states.
CREATE TABLE chat_statement (
    id                  uuid        PRIMARY KEY,
    station_id          uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id          uuid        REFERENCES chat_account (id) ON DELETE CASCADE,
    action_id           bigint      REFERENCES chat_action (id) ON DELETE SET NULL,
    at                  timestamptz NOT NULL DEFAULT now(),
    text                text        NOT NULL CHECK (length(text) BETWEEN 1 AND 4000),
    automated           boolean     NOT NULL DEFAULT false,
    review_by           timestamptz NOT NULL,
    review_requested_at timestamptz
);
CREATE INDEX chat_statement_account ON chat_statement (account_id);

-- A time-out or a ban. A ban has no end; a time-out ends at `until`.
CREATE TABLE chat_restriction (
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id  uuid        NOT NULL REFERENCES chat_account (id) ON DELETE CASCADE,
    kind        text        NOT NULL CHECK (kind IN ('timeout', 'ban')),
    until       timestamptz,
    reason      text        NOT NULL DEFAULT '' CHECK (length(reason) <= 40),
    at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (station_id, account_id)
);

-- An evidence hold (ADR-0116's shape): the records it names are not deleted until it is
-- released. Opened by "preserve evidence and escalate".
CREATE TABLE chat_hold (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    report_id   uuid        REFERENCES chat_report (id) ON DELETE SET NULL,
    account_id  uuid        REFERENCES chat_account (id) ON DELETE SET NULL,
    message_ids bigint[]    NOT NULL DEFAULT '{}',
    reason      text        NOT NULL CHECK (length(reason) BETWEEN 1 AND 300),
    held_by     text        NOT NULL CHECK (length(held_by) BETWEEN 1 AND 120),
    held_at     timestamptz NOT NULL DEFAULT now(),
    released_at timestamptz,
    released_by text
);

-- The station's chat settings the station edits: the legal fields (terms, the accountable
-- person, the contact point, the minimum age), the mode (full or comments only, ADR-0035),
-- guest posting, and its own blocked words and names. One JSON document per station.
CREATE TABLE chat_settings (
    station_id  uuid        PRIMARY KEY REFERENCES station (id) ON DELETE CASCADE,
    settings    jsonb       NOT NULL DEFAULT '{}',
    updated_at  timestamptz NOT NULL DEFAULT now()
);
