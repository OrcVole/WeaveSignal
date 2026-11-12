-- SPDX-License-Identifier: AGPL-3.0-only
-- A message may carry one picture or document (ADR-0262): `attachment` is its `chat_avatar` row
-- (migration 0031). A message with one is stored `held` and stays so until staff or a moderator
-- approve the attachment; it is never in the room's hold for new chatters, and only approving the
-- attachment releases it. Its words may be empty: the attachment is the message.
--
-- No foreign key: a picture kept as evidence must outlive its message's retention, and a message
-- whose attachment was deleted reads as one without it. The sweep deletes the rows no message
-- carries (except withheld ones), so an attachment goes when its message goes.
ALTER TABLE chat_message ADD COLUMN attachment uuid UNIQUE;
ALTER TABLE chat_message DROP CONSTRAINT chat_message_body_check;
ALTER TABLE chat_message ADD CONSTRAINT chat_message_body_check
    CHECK (length(body) <= 2000 AND (length(body) >= 1 OR attachment IS NOT NULL));
