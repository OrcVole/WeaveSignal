-- SPDX-License-Identifier: AGPL-3.0-only
-- Chatters have avatars (ADR-0235): one of the set WeaveSignal ships, chosen by its id
-- (`packages/ui/avatars/manifest.json`), or a picture of their own, which nobody else sees
-- until staff approve it.
--
-- `chat_account.avatar_stock` is the shipped avatar a chatter chose; null is the plain default
-- (their initial).
--
-- `chat_avatar` is an uploaded picture, as the chat's media worker rewrote it (a small PNG with
-- nothing but pixels). The file itself is on disk, `<avatars.dir>/<id>.png`; the row says whose
-- it is and where it stands:
--   held      waiting for staff; only its owner and staff can fetch it. At most one per account:
--             a new upload replaces it.
--   approved  everyone sees it beside the chatter's messages. At most one per account.
--   withheld  refused or removed, and kept only because an evidence hold names the account
--             (`chat_hold`, as for messages); nobody but staff can fetch it, and the sweep deletes
--             it once no open hold names the account.
-- A picture that is refused or removed with no hold is deleted at once, row and file. Erasing
-- the account deletes its pictures (the row would go with the account anyway; the chat removes
-- the files).

ALTER TABLE chat_account
    ADD COLUMN avatar_stock text CHECK (avatar_stock IS NULL OR length(avatar_stock) BETWEEN 1 AND 40);

CREATE TABLE chat_avatar (
    id          uuid        PRIMARY KEY,
    station_id  uuid        NOT NULL REFERENCES station (id) ON DELETE CASCADE,
    account_id  uuid        NOT NULL REFERENCES chat_account (id) ON DELETE CASCADE,
    state       text        NOT NULL CHECK (state IN ('held', 'approved', 'withheld')),
    bytes       integer     NOT NULL CHECK (bytes > 0),
    side        smallint    NOT NULL CHECK (side > 0),
    uploaded_at timestamptz NOT NULL DEFAULT now(),
    decided_at  timestamptz,
    decided_by  text        CHECK (decided_by IS NULL OR length(decided_by) <= 120)
);
CREATE UNIQUE INDEX chat_avatar_one ON chat_avatar (account_id, state)
    WHERE state IN ('held', 'approved');
CREATE INDEX chat_avatar_queue ON chat_avatar (station_id, uploaded_at) WHERE state = 'held';
