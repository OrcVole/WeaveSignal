-- SPDX-License-Identifier: AGPL-3.0-only
-- Pictures and documents posted in the chat (ADR-0258, ADR-0262), kept in the same table as
-- chatters' uploaded avatars so that one queue, one evidence path and one retention cover both
-- (ADR-0235, ADR-0254, ADR-0257): the states `held`, `approved` and `withheld` mean what they mean
-- for an avatar, a refused or removed one is deleted unless an evidence hold names the account,
-- and one refused as suspected child sexual abuse material is kept with its `evidence` snapshot.
--
-- `kind` says which: `avatar` (every row before this), `picture` or `document`. A picture is
-- rewritten by the media worker: `<attachments.dir>/<id>.full` (a PNG or a JPEG, `mime` says
-- which) and its thumbnail `<id>.thumb` (`thumb_mime`), beside the upload as it arrived,
-- `<id>.orig`, never served. A document is passed on unchanged, only as a download: its one file is
-- `<id>.orig`, of the type its bytes showed (`mime`), never what its name said.
-- `file_name` is the name the chatter's file had, shown as text only.
--
-- A picture or document belongs to the message that carries it (`chat_message.attachment`,
-- migration 0032) and goes when that message goes; the sweep deletes one no message carries,
-- unless it is withheld.
ALTER TABLE chat_avatar
    ADD COLUMN kind         text    NOT NULL DEFAULT 'avatar'
                                    CHECK (kind IN ('avatar', 'picture', 'document')),
    ADD COLUMN mime         text    CHECK (mime IS NULL OR length(mime) BETWEEN 1 AND 80),
    ADD COLUMN file_name    text    CHECK (file_name IS NULL OR length(file_name) BETWEEN 1 AND 120),
    ADD COLUMN width        integer CHECK (width IS NULL OR width > 0),
    ADD COLUMN height       integer CHECK (height IS NULL OR height > 0),
    ADD COLUMN thumb_mime   text    CHECK (thumb_mime IS NULL OR length(thumb_mime) BETWEEN 1 AND 80),
    ADD COLUMN thumb_width  integer CHECK (thumb_width IS NULL OR thumb_width > 0),
    ADD COLUMN thumb_height integer CHECK (thumb_height IS NULL OR thumb_height > 0),
    ADD COLUMN thumb_bytes  integer CHECK (thumb_bytes IS NULL OR thumb_bytes > 0);

-- A document has no side.
ALTER TABLE chat_avatar ALTER COLUMN side DROP NOT NULL;
ALTER TABLE chat_avatar ADD CONSTRAINT chat_avatar_kind_fields CHECK (
    (kind = 'avatar' AND side IS NOT NULL)
    OR (kind = 'picture' AND mime IS NOT NULL AND width IS NOT NULL AND height IS NOT NULL
        AND thumb_mime IS NOT NULL)
    OR (kind = 'document' AND mime IS NOT NULL AND file_name IS NOT NULL));

-- One avatar waiting and one showing per account, as before; a chatter may post many pictures.
DROP INDEX chat_avatar_one;
CREATE UNIQUE INDEX chat_avatar_one ON chat_avatar (account_id, state)
    WHERE kind = 'avatar' AND state IN ('held', 'approved');
