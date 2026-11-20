-- SPDX-License-Identifier: AGPL-3.0-only

-- WHIP for remote DJs (17d): a third way in beside Icecast and SRT. A host's OBS sends over WHIP
-- with the show key's id in the address and its secret as the bearer; the ingest answers it on
-- one UDP port of its own, ICE-lite, sound only. The key's hint and the log's way gain `whip`
-- (beside 0042's `pull`; numbered 0050 on branch `whip`, renumbered when the 1.2 branches merged), and
-- the station's rules gain WHIP's port (UDP 9712 beside SRT's 9710; the installation's own
-- setting wins, and WHIP is off on every route until the installation turns it on).
ALTER TABLE show_key DROP CONSTRAINT show_key_hint_check;
ALTER TABLE show_key ADD CONSTRAINT show_key_hint_check CHECK (hint IN ('icecast', 'srt', 'pull', 'whip'));

ALTER TABLE source_log DROP CONSTRAINT source_log_way_check;
ALTER TABLE source_log ADD CONSTRAINT source_log_way_check CHECK (way IN ('icecast', 'srt', 'pull', 'whip'));

ALTER TABLE source_settings
    ADD COLUMN port_whip integer NOT NULL DEFAULT 9712 CHECK (port_whip BETWEEN 1 AND 65535);
