-- SPDX-License-Identifier: AGPL-3.0-only
-- Pictures that are kept as evidence, and the originals (ADR-0257).
--
-- `has_original` says the upload as it arrived is on disk beside the clean PNG,
-- `<avatars.dir>/<id>.orig`, for as long as the row lives: a report of child sexual abuse
-- material needs the original, and a picture can be found out after it was approved. It is never
-- served; only the evidence export carries it.
--
-- `upload_address` is the uploader's address, kept only where the station's retention keeps
-- addresses (none by default, ADR-0115) and cleared by the same sweep.
--
-- `reason` is why a withheld picture is kept (the report reason, as `chat_action.reason`).
-- `evidence` is set only for a picture refused as suspected child sexual abuse material: a
-- snapshot of the uploader and the decision, made when it was decided so that an erasure, an
-- address sweep or the moderation record's own retention cannot take it. `keep_until` is the
-- earliest the sweep may delete that picture, whatever happens to the evidence hold that
-- withheld it (the law asks for a year); staff may still delete it at once, after a confirmation
-- that is logged.
ALTER TABLE chat_avatar
    ADD COLUMN has_original   boolean     NOT NULL DEFAULT false,
    ADD COLUMN upload_address inet,
    ADD COLUMN reason         text        CHECK (reason IS NULL OR length(reason) <= 40),
    ADD COLUMN keep_until     timestamptz,
    ADD COLUMN evidence       jsonb;

CREATE INDEX chat_avatar_kept ON chat_avatar (station_id, decided_at) WHERE state = 'withheld';
